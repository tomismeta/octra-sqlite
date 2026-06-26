use super::{
    error::{ClientError, ClientErrorKind, Result},
    session::{build_session, Session, SessionOptions},
    transport::{HttpTransport, Transport},
};
use crate::protocol::{
    osr1::{decode_typed_result, TYPED_PREFIX},
    osw1,
    tx::{canonical_tx, Tx},
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::env;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct OctraSqlite<T = HttpTransport> {
    options: SessionOptions,
    transport: Arc<T>,
}

impl Default for OctraSqlite<HttpTransport> {
    fn default() -> Self {
        Self {
            options: SessionOptions::default(),
            transport: Arc::new(HttpTransport::default()),
        }
    }
}

impl OctraSqlite<HttpTransport> {
    pub fn from_default_config() -> Result<Self> {
        super::config::load_config()?;
        Ok(Self::default())
    }

    pub fn with_options(options: SessionOptions) -> Self {
        Self {
            options,
            transport: Arc::new(HttpTransport::default()),
        }
    }
}

impl<T: Transport> OctraSqlite<T> {
    pub fn with_transport(options: SessionOptions, transport: T) -> Self {
        Self {
            options,
            transport: Arc::new(transport),
        }
    }

    pub fn database(&self, target: impl Into<String>) -> Result<Database<T>> {
        let mut options = self.options.clone();
        options.target = Some(target.into());
        Database::open_with_shared_transport(options, Arc::clone(&self.transport))
    }
}

#[derive(Clone)]
pub struct Database<T = HttpTransport> {
    session: Session,
    transport: Arc<T>,
}

impl Database<HttpTransport> {
    pub fn open(options: SessionOptions) -> Result<Self> {
        Self::open_with_transport(options, HttpTransport::default())
    }
}

impl<T: Transport> Database<T> {
    pub fn open_with_transport(options: SessionOptions, transport: T) -> Result<Self> {
        Self::open_with_shared_transport(options, Arc::new(transport))
    }

    fn open_with_shared_transport(options: SessionOptions, transport: Arc<T>) -> Result<Self> {
        Ok(Self {
            session: build_session(&options)?,
            transport,
        })
    }

    pub fn query(&self, sql: &str) -> Result<QueryResult> {
        QueryResult::from_value(query_typed_with(
            self.transport.as_ref(),
            &self.session,
            sql,
        )?)
    }

    pub fn execute(&self, sql: &str) -> Result<ExecResult> {
        let prepared = self.prepare_write(sql)?;
        let signed = self.sign_write(&prepared)?;
        self.submit_signed_write_and_wait(signed)
    }

    pub fn execute_no_wait(&self, sql: &str) -> Result<SubmittedTx> {
        let prepared = self.prepare_write_no_wait(sql)?;
        let signed = self.sign_write(&prepared)?;
        self.submit_signed_write(signed)
    }

    pub fn prepare_write_no_wait(&self, sql: &str) -> Result<PreparedWrite> {
        self.prepare_write_for(sql, DatabaseOperation::ExecuteNoWait)
    }

    pub fn prepare_write(&self, sql: &str) -> Result<PreparedWrite> {
        self.prepare_write_for(sql, DatabaseOperation::Execute)
    }

    fn prepare_write_for(&self, sql: &str, operation: DatabaseOperation) -> Result<PreparedWrite> {
        prepare_write_with(self.transport.as_ref(), &self.session, sql, operation)
    }

    pub fn sign_write(&self, prepared: &PreparedWrite) -> Result<SignedWrite> {
        sign_write(&self.session, prepared)
    }

    pub fn submit_signed_write(&self, signed: SignedWrite) -> Result<SubmittedTx> {
        ensure_submit_mode(&signed, DatabaseOperation::ExecuteNoWait)?;
        SubmittedTx::from_value(submit_signed_write_with(
            self.transport.as_ref(),
            &self.session,
            signed,
            true,
        )?)
    }

    pub fn submit_signed_write_and_wait(&self, signed: SignedWrite) -> Result<ExecResult> {
        ensure_submit_mode(&signed, DatabaseOperation::Execute)?;
        ExecResult::from_value(submit_signed_write_with(
            self.transport.as_ref(),
            &self.session,
            signed,
            false,
        )?)
    }

    pub fn auth_info(&self) -> Result<AuthInfo> {
        auth_info_with(self.transport.as_ref(), &self.session)
    }

    pub fn program_info(&self) -> Result<ProgramInfo> {
        ProgramInfo::from_value(program_info_with(self.transport.as_ref(), &self.session)?)
    }
}

fn ensure_submit_mode(signed: &SignedWrite, expected: DatabaseOperation) -> Result<()> {
    let actual = signed.safety.operation;
    if actual != expected {
        return Err(ClientError::with_kind(
            ClientErrorKind::Config,
            format!("signed write was prepared for {actual:?}, not {expected:?}"),
        ));
    }
    Ok(())
}

fn ensure_prepared_for_session(session: &Session, prepared: &PreparedWrite) -> Result<()> {
    if prepared.circle != session.target().circle {
        return Err(ClientError::with_kind(
            ClientErrorKind::Authorization,
            "prepared write Circle does not match the active database",
        ));
    }
    if prepared.wallet != session.caller() {
        return Err(ClientError::with_kind(
            ClientErrorKind::Authorization,
            "prepared write wallet does not match the active session",
        ));
    }
    if prepared.public_key != session.public_key_b64() {
        return Err(ClientError::with_kind(
            ClientErrorKind::Authorization,
            "prepared write public key does not match the active session",
        ));
    }
    Ok(())
}

fn ensure_signed_for_session(session: &Session, signed: &SignedWrite) -> Result<()> {
    if signed.tx.to_ != session.target().circle {
        return Err(ClientError::with_kind(
            ClientErrorKind::Authorization,
            "signed write Circle does not match the active database",
        ));
    }
    if signed.tx.from != session.caller() {
        return Err(ClientError::with_kind(
            ClientErrorKind::Authorization,
            "signed write wallet does not match the active session",
        ));
    }
    if signed.tx.public_key != session.public_key_b64() {
        return Err(ClientError::with_kind(
            ClientErrorKind::Authorization,
            "signed write public key does not match the active session",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseOperation {
    Query,
    Execute,
    ExecuteNoWait,
    AuthInfo,
    ProgramInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSafety {
    pub operation: DatabaseOperation,
    pub reads_sql: bool,
    pub mutates_state: bool,
    pub submits_transaction: bool,
    pub waits_for_receipt: bool,
    pub requires_signed_rpc: bool,
    pub requires_owner_write_intent: bool,
}

pub fn operation_safety(operation: DatabaseOperation) -> OperationSafety {
    match operation {
        DatabaseOperation::Query => OperationSafety {
            operation,
            reads_sql: true,
            mutates_state: false,
            submits_transaction: false,
            waits_for_receipt: false,
            requires_signed_rpc: true,
            requires_owner_write_intent: false,
        },
        DatabaseOperation::Execute => OperationSafety {
            operation,
            reads_sql: true,
            mutates_state: true,
            submits_transaction: true,
            waits_for_receipt: true,
            requires_signed_rpc: true,
            requires_owner_write_intent: true,
        },
        DatabaseOperation::ExecuteNoWait => OperationSafety {
            operation,
            reads_sql: true,
            mutates_state: true,
            submits_transaction: true,
            waits_for_receipt: false,
            requires_signed_rpc: true,
            requires_owner_write_intent: true,
        },
        DatabaseOperation::AuthInfo | DatabaseOperation::ProgramInfo => OperationSafety {
            operation,
            reads_sql: false,
            mutates_state: false,
            submits_transaction: false,
            waits_for_receipt: false,
            requires_signed_rpc: true,
            requires_owner_write_intent: false,
        },
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub row_count: usize,
    raw: Value,
}

impl QueryResult {
    pub fn from_value(value: Value) -> Result<Self> {
        let columns = value
            .get("columns")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ClientError::with_kind(ClientErrorKind::Decode, "query result missing columns")
            })?
            .iter()
            .map(|column| {
                column.as_str().map(str::to_string).ok_or_else(|| {
                    ClientError::with_kind(
                        ClientErrorKind::Decode,
                        "query result column must be a string",
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let rows = value
            .get("rows")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ClientError::with_kind(ClientErrorKind::Decode, "query result missing rows")
            })?
            .iter()
            .map(|row| {
                row.as_array().cloned().ok_or_else(|| {
                    ClientError::with_kind(
                        ClientErrorKind::Decode,
                        "query result row must be an array",
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let row_count = value
            .get("row_count")
            .and_then(Value::as_u64)
            .map(|count| count as usize)
            .unwrap_or(rows.len());
        if row_count != rows.len() {
            return Err(ClientError::with_kind(
                ClientErrorKind::Decode,
                format!(
                    "query result row_count {row_count} does not match {} rows",
                    rows.len()
                ),
            ));
        }
        for row in &rows {
            if row.len() != columns.len() {
                return Err(ClientError::with_kind(
                    ClientErrorKind::Decode,
                    format!(
                        "query result row has {} cells but {} columns",
                        row.len(),
                        columns.len()
                    ),
                ));
            }
        }
        Ok(Self {
            columns,
            rows,
            row_count,
            raw: value,
        })
    }

    pub fn raw(&self) -> &Value {
        &self.raw
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubmittedTx {
    pub circle: Option<String>,
    pub wallet: Option<String>,
    pub tx_hash: Option<String>,
    pub result: Value,
}

impl SubmittedTx {
    pub fn from_value(value: Value) -> Result<Self> {
        Ok(Self {
            circle: string_field(&value, "circle"),
            wallet: string_field(&value, "wallet"),
            tx_hash: string_field(&value, "tx_hash"),
            result: value.get("result").cloned().ok_or_else(|| {
                ClientError::with_kind(ClientErrorKind::Rpc, "submitted transaction missing result")
            })?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecResult {
    pub submitted: SubmittedTx,
    pub receipt: Value,
}

impl ExecResult {
    pub fn from_value(value: Value) -> Result<Self> {
        let submitted = SubmittedTx::from_value(value.clone())?;
        let receipt = value.get("receipt").cloned().ok_or_else(|| {
            ClientError::with_kind(ClientErrorKind::Receipt, "exec result missing receipt")
        })?;
        ensure_receipt_success(&receipt)?;
        Ok(Self { submitted, receipt })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgramInfo {
    pub version: Option<String>,
    pub code_hash: Option<String>,
    pub code_bytes: Option<u64>,
    raw: Value,
}

impl ProgramInfo {
    pub fn from_value(value: Value) -> Result<Self> {
        Ok(Self {
            version: string_field(&value, "version"),
            code_hash: string_field(&value, "code_hash"),
            code_bytes: value
                .get("code_bytes")
                .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok())),
            raw: value,
        })
    }

    pub fn raw(&self) -> &Value {
        &self.raw
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthInfo {
    pub configured: bool,
    pub db_id: String,
    pub owner_pubkey: Option<String>,
    pub owner_sequence: Option<u64>,
}

#[derive(Clone, PartialEq)]
pub struct PreparedWrite {
    sql: String,
    method: String,
    nonce: i64,
    timestamp: f64,
    circle: String,
    wallet: String,
    public_key: String,
    owner_write: PreparedOwnerWrite,
    safety: OperationSafety,
}

impl PreparedWrite {
    pub fn sql(&self) -> &str {
        &self.sql
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn nonce(&self) -> i64 {
        self.nonce
    }

    pub fn timestamp(&self) -> f64 {
        self.timestamp
    }

    pub fn circle(&self) -> &str {
        &self.circle
    }

    pub fn wallet(&self) -> &str {
        &self.wallet
    }

    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    pub fn owner_write(&self) -> &PreparedOwnerWrite {
        &self.owner_write
    }

    pub fn safety(&self) -> OperationSafety {
        self.safety
    }
}

impl fmt::Debug for PreparedWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedWrite")
            .field("method", &self.method)
            .field("nonce", &self.nonce)
            .field("circle", &self.circle)
            .field("wallet", &self.wallet)
            .field("owner_write", &self.owner_write)
            .field("safety", &self.safety)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PreparedOwnerWrite {
    db_id: String,
    owner_pubkey: String,
    sequence: u64,
    frame_hex: String,
}

impl PreparedOwnerWrite {
    pub fn db_id(&self) -> &str {
        &self.db_id
    }

    pub fn owner_pubkey(&self) -> &str {
        &self.owner_pubkey
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn frame_hex(&self) -> &str {
        &self.frame_hex
    }
}

impl fmt::Debug for PreparedOwnerWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedOwnerWrite")
            .field("db_id", &self.db_id)
            .field("owner_pubkey", &self.owner_pubkey)
            .field("sequence", &self.sequence)
            .field("frame_hex", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq)]
pub struct SignedWrite {
    tx: Tx,
    safety: OperationSafety,
}

impl SignedWrite {
    pub fn tx(&self) -> &Tx {
        &self.tx
    }

    pub fn safety(&self) -> OperationSafety {
        self.safety
    }

    pub fn into_tx(self) -> Tx {
        self.tx
    }
}

impl fmt::Debug for SignedWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignedWrite")
            .field("circle", &self.tx.to_)
            .field("wallet", &self.tx.from)
            .field("nonce", &self.tx.nonce)
            .field("method", &self.tx.encrypted_data)
            .field("safety", &self.safety)
            .finish_non_exhaustive()
    }
}

pub fn view(session: &Session, method: &str, params: Vec<Value>) -> Result<Value> {
    let transport = HttpTransport::default();
    view_with(&transport, session, method, params)
}

pub fn query_typed(session: &Session, sql: &str) -> Result<Value> {
    let transport = HttpTransport::default();
    query_typed_with(&transport, session, sql)
}

pub fn auth_info(session: &Session) -> Result<AuthInfo> {
    let transport = HttpTransport::default();
    auth_info_with(&transport, session)
}

pub fn program_info(session: &Session) -> Result<Value> {
    let transport = HttpTransport::default();
    program_info_with(&transport, session)
}

pub fn exec_sql(session: &Session, sql: &str, no_wait: bool) -> Result<Value> {
    let transport = HttpTransport::default();
    let operation = if no_wait {
        DatabaseOperation::ExecuteNoWait
    } else {
        DatabaseOperation::Execute
    };
    let prepared = prepare_write_with(&transport, session, sql, operation)?;
    let signed = sign_write(session, &prepared)?;
    submit_signed_write_with(&transport, session, signed, no_wait)
}

pub fn next_nonce(session: &Session) -> Result<i64> {
    let transport = HttpTransport::default();
    next_nonce_with(&transport, session)
}

pub fn submit_tx(session: &Session, mut tx: Tx, no_wait: bool) -> Result<Value> {
    let transport = HttpTransport::default();
    let canonical = canonical_tx(&tx);
    tx.signature = session.sign_text_b64(&canonical)?;
    let signed = SignedWrite {
        tx,
        safety: operation_safety(if no_wait {
            DatabaseOperation::ExecuteNoWait
        } else {
            DatabaseOperation::Execute
        }),
    };
    submit_signed_write_with(&transport, session, signed, no_wait)
}

pub fn wait_for_transaction(session: &Session, tx_hash: &str) -> Result<Value> {
    let transport = HttpTransport::default();
    wait_for_transaction_with(&transport, session, tx_hash)
}

fn view_with<T: Transport>(
    transport: &T,
    session: &Session,
    method: &str,
    params: Vec<Value>,
) -> Result<Value> {
    let params_value = Value::Array(params.clone());
    let params_json = compact_json(&params_value)?;
    let params_hash = sha256_hex(params_json.as_bytes());
    let message = format!(
        "octra_circle_view|{}|{}|{}|{}|0",
        session.target().circle,
        session.caller(),
        method,
        params_hash
    );
    let signature = session.sign_text_b64(&message)?;
    let result = rpc_call(
        transport,
        session,
        "octra_circleViewAuth",
        json!([
            session.target().circle,
            method,
            params,
            session.caller(),
            session.public_key_b64(),
            signature,
            false
        ]),
    )?;
    decode_rpc_result(result)
}

fn query_typed_with<T: Transport>(transport: &T, session: &Session, sql: &str) -> Result<Value> {
    view_with(
        transport,
        session,
        "query_typed",
        vec![Value::String(sql.to_string())],
    )
}

fn auth_info_with<T: Transport>(transport: &T, session: &Session) -> Result<AuthInfo> {
    let value = view_with(transport, session, "auth_info", vec![])?;
    let configured = value
        .get("configured")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let db_id = value
        .get("db_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ClientError::with_kind(ClientErrorKind::Decode, "auth_info missing db_id"))?
        .to_string();
    let owner_pubkey = value
        .get("owner_pubkey")
        .and_then(Value::as_str)
        .map(str::to_string);
    let owner_sequence = value.get("owner_sequence").and_then(Value::as_u64);
    Ok(AuthInfo {
        configured,
        db_id,
        owner_pubkey,
        owner_sequence,
    })
}

fn program_info_with<T: Transport>(transport: &T, session: &Session) -> Result<Value> {
    let message = format!(
        "octra_circle_program_info|{}|{}",
        session.target().circle,
        session.caller()
    );
    let signature = session.sign_text_b64(&message)?;
    rpc_call(
        transport,
        session,
        "octra_circleProgramInfoAuth",
        json!([
            session.target().circle,
            session.caller(),
            session.public_key_b64(),
            signature
        ]),
    )
}

fn prepare_write_with<T: Transport>(
    transport: &T,
    session: &Session,
    sql: &str,
    operation: DatabaseOperation,
) -> Result<PreparedWrite> {
    let nonce = next_nonce_with(transport, session)?;
    let timestamp = now_timestamp();
    let method = if trace_sql_event_enabled() {
        "exec_trace"
    } else {
        "exec"
    };
    let auth = auth_info_with(transport, session).map_err(|error| {
        ClientError::with_kind(
            ClientErrorKind::Authorization,
            format!(
                "could not read Circle auth_info; refusing to choose unsigned exec implicitly: {error}"
            ),
        )
    })?;
    if !auth.configured {
        return Err(ClientError::with_kind(
            ClientErrorKind::Authorization,
            "database is not owner-write-personalized; refusing unsigned SQL write",
        ));
    }
    let db_id = hex_to_32("db_id", &auth.db_id)?;
    let owner_pubkey = hex::encode(session.intent_public_key()?);
    let frame = osw1::frame(&db_id, nonce as u64, method, sql)?;
    let owner_write = PreparedOwnerWrite {
        db_id: auth.db_id,
        owner_pubkey,
        sequence: nonce as u64,
        frame_hex: hex::encode(frame),
    };
    Ok(PreparedWrite {
        sql: sql.to_string(),
        method: method.to_string(),
        nonce,
        timestamp,
        circle: session.target().circle.clone(),
        wallet: session.caller().to_string(),
        public_key: session.public_key_b64().to_string(),
        owner_write,
        safety: operation_safety(operation),
    })
}

fn sign_write(session: &Session, prepared: &PreparedWrite) -> Result<SignedWrite> {
    ensure_prepared_for_session(session, prepared)?;
    let owner_write = &prepared.owner_write;
    let params = vec![
        Value::String(prepared.sql.clone()),
        Value::String(owner_write.owner_pubkey.clone()),
        Value::String(owner_write.sequence.to_string()),
        Value::String(session.sign_bytes_hex(&hex::decode(&owner_write.frame_hex)?)?),
    ];
    let message = compact_json(&Value::Array(params))?;
    let mut tx = Tx {
        from: prepared.wallet.clone(),
        to_: prepared.circle.clone(),
        amount: "0".to_string(),
        nonce: prepared.nonce,
        ou: "1000".to_string(),
        timestamp: prepared.timestamp,
        op_type: "circle_call".to_string(),
        encrypted_data: prepared.method.clone(),
        message,
        signature: String::new(),
        public_key: prepared.public_key.clone(),
    };
    tx.signature = session.sign_text_b64(&canonical_tx(&tx))?;
    Ok(SignedWrite {
        tx,
        safety: prepared.safety,
    })
}

fn next_nonce_with<T: Transport>(transport: &T, session: &Session) -> Result<i64> {
    let balance = rpc_call(
        transport,
        session,
        "octra_balance",
        json!([session.caller()]),
    )?;
    Ok(balance
        .get("pending_nonce")
        .or_else(|| balance.get("nonce"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        + 1)
}

fn submit_signed_write_with<T: Transport>(
    transport: &T,
    session: &Session,
    signed: SignedWrite,
    no_wait: bool,
) -> Result<Value> {
    ensure_signed_for_session(session, &signed)?;
    let tx_circle = signed.tx.to_.clone();
    let tx_wallet = signed.tx.from.clone();
    let result = rpc_call(transport, session, "octra_submit", json!([signed.tx]))?;
    let tx_hash = result
        .get("tx_hash")
        .or_else(|| result.get("hash"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut out = Map::new();
    out.insert("circle".to_string(), Value::String(tx_circle));
    out.insert("wallet".to_string(), Value::String(tx_wallet));
    out.insert("result".to_string(), result);
    if let Some(hash) = tx_hash.clone() {
        out.insert("tx_hash".to_string(), Value::String(hash.clone()));
        if !no_wait {
            let receipt = wait_for_receipt_with(transport, session, &hash)?;
            ensure_receipt_success(&receipt)?;
            out.insert("receipt".to_string(), receipt);
        }
    }
    Ok(Value::Object(out))
}

fn rpc_call<T: Transport>(
    transport: &T,
    session: &Session,
    method: &str,
    params: Value,
) -> Result<Value> {
    transport.call(session.rpc(), method, params)
}

fn wait_for_receipt_with<T: Transport>(
    transport: &T,
    session: &Session,
    tx_hash: &str,
) -> Result<Value> {
    for _ in 0..45 {
        let result = rpc_call(transport, session, "contract_receipt", json!([tx_hash]));
        if let Ok(receipt) = result {
            if !receipt.is_null() {
                return Ok(receipt);
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err(ClientError::with_kind(
        ClientErrorKind::Timeout,
        format!("timed out waiting for receipt {tx_hash}"),
    ))
}

fn ensure_receipt_success(receipt: &Value) -> Result<()> {
    let failed = receipt.get("success").and_then(Value::as_bool) == Some(false)
        || receipt.get("error").is_some_and(|error| !error.is_null());
    if failed {
        return Err(ClientError::with_kind(
            ClientErrorKind::Receipt,
            format!("SQL execution failed: {}", receipt_error_text(receipt)),
        ));
    }
    Ok(())
}

fn receipt_error_text(receipt: &Value) -> String {
    receipt
        .get("error")
        .filter(|error| !error.is_null())
        .map(value_to_compact_text)
        .unwrap_or_else(|| value_to_compact_text(receipt))
}

fn wait_for_transaction_with<T: Transport>(
    transport: &T,
    session: &Session,
    tx_hash: &str,
) -> Result<Value> {
    for _ in 0..60 {
        let result = rpc_call(transport, session, "octra_transaction", json!([tx_hash]));
        if let Ok(transaction) = result {
            let status = transaction
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match status {
                "confirmed" | "accepted" => return Ok(transaction),
                "rejected" | "failed" => {
                    return Err(ClientError::with_kind(
                        ClientErrorKind::Receipt,
                        format!("transaction {tx_hash} {status}: {transaction}"),
                    ));
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err(ClientError::with_kind(
        ClientErrorKind::Timeout,
        format!("timed out waiting for transaction {tx_hash}"),
    ))
}

fn decode_rpc_result(result: Value) -> Result<Value> {
    if let Some(text) = result.get("result").and_then(Value::as_str) {
        return decode_method_result(text);
    }
    if let Some(text) = result.as_str() {
        return decode_method_result(text);
    }
    Ok(result)
}

fn decode_method_result(text: &str) -> Result<Value> {
    if let Some(encoded) = text.strip_prefix(TYPED_PREFIX) {
        return Ok(decode_typed_result(encoded)?);
    }
    Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string())))
}

fn compact_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn value_to_compact_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn hex_to_32(label: &str, text: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(text).map_err(|error| {
        ClientError::with_kind(
            ClientErrorKind::Decode,
            format!("decoding {label} hex: {error}"),
        )
    })?;
    if bytes.len() != 32 {
        return Err(ClientError::with_kind(
            ClientErrorKind::Decode,
            format!("{label} must decode to 32 bytes"),
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn trace_sql_event_enabled() -> bool {
    env::var("OCTRA_SQLITE_TRACE_SQL_EVENT")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn now_timestamp() -> f64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_secs() as f64 + f64::from(duration.subsec_millis()) / 1000.0
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockTransport {
        calls: Arc<Mutex<Vec<String>>>,
        receipt: Arc<Mutex<Value>>,
    }

    impl Default for MockTransport {
        fn default() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                receipt: Arc::new(Mutex::new(json!({
                    "success": true,
                    "error": null,
                    "method": "exec",
                }))),
            }
        }
    }

    impl MockTransport {
        fn with_receipt(receipt: Value) -> Self {
            Self {
                receipt: Arc::new(Mutex::new(receipt)),
                ..Self::default()
            }
        }
    }

    impl Transport for MockTransport {
        fn call(&self, _rpc: &str, method: &str, params: Value) -> Result<Value> {
            self.calls.lock().unwrap().push(method.to_string());
            match method {
                "octra_circleViewAuth" => {
                    let circle_method = params
                        .as_array()
                        .and_then(|params| params.get(1))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if circle_method == "auth_info" {
                        return Ok(json!({
                            "configured": true,
                            "db_id": "1111111111111111111111111111111111111111111111111111111111111111",
                        }));
                    }
                    let vector: Value =
                        serde_json::from_str(include_str!("../../tests/fixtures/osr1/basic.json"))
                            .unwrap();
                    Ok(Value::String(format!(
                        "OSR1:{}",
                        vector["payload_b64"].as_str().unwrap()
                    )))
                }
                "octra_balance" => Ok(json!({ "pending_nonce": 41 })),
                "octra_submit" => Ok(json!({ "tx_hash": "abc123" })),
                "contract_receipt" => Ok(self.receipt.lock().unwrap().clone()),
                _ => Err(ClientError::with_kind(
                    ClientErrorKind::Other,
                    format!("unexpected method {method}"),
                )),
            }
        }
    }

    fn test_options() -> SessionOptions {
        SessionOptions {
            target: Some("oct://devnet/octABC".to_string()),
            rpc: Some("mock://rpc".to_string()),
            caller: Some("octCaller".to_string()),
            private_key: Some(
                "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
            ),
            ..SessionOptions::default()
        }
    }

    fn test_options_for(target: &str) -> SessionOptions {
        SessionOptions {
            target: Some(target.to_string()),
            ..test_options()
        }
    }

    #[test]
    fn database_query_uses_transport_and_returns_typed_rows() {
        let transport = MockTransport::default();
        let calls = transport.calls.clone();
        let db = Database::open_with_transport(test_options(), transport).unwrap();
        let result = db.query("select * from demo").unwrap();
        assert_eq!(result.columns[0], "nil");
        assert_eq!(result.row_count, 1);
        assert_eq!(calls.lock().unwrap().as_slice(), ["octra_circleViewAuth"]);
    }

    #[test]
    fn database_execute_errors_on_failed_receipt() {
        let transport = MockTransport::with_receipt(json!({
            "success": false,
            "error": "near \"bad\": syntax error",
            "method": "exec",
        }));
        let db = Database::open_with_transport(test_options(), transport).unwrap();
        let error = db.execute("bad sql").unwrap_err();
        assert_eq!(error.kind(), ClientErrorKind::Receipt);
        assert!(error.to_string().contains("syntax error"));
    }

    #[test]
    fn signed_write_submit_mode_must_match_prepare_mode() {
        let transport = MockTransport::default();
        let db = Database::open_with_transport(test_options(), transport).unwrap();
        let prepared = db.prepare_write("create table demo(id integer);").unwrap();
        let signed = db.sign_write(&prepared).unwrap();
        let error = db.submit_signed_write(signed).unwrap_err();
        assert_eq!(error.kind(), ClientErrorKind::Config);
    }

    #[test]
    fn prepared_write_must_match_signing_database() {
        let db_a = Database::open_with_transport(
            test_options_for("oct://devnet/octABC"),
            MockTransport::default(),
        )
        .unwrap();
        let db_b = Database::open_with_transport(
            test_options_for("oct://devnet/octDEF"),
            MockTransport::default(),
        )
        .unwrap();
        let prepared = db_a
            .prepare_write("create table demo(id integer);")
            .unwrap();
        let error = db_b.sign_write(&prepared).unwrap_err();
        assert_eq!(error.kind(), ClientErrorKind::Authorization);
    }

    #[test]
    fn query_result_validates_rectangular_rows() {
        let error = QueryResult::from_value(json!({
            "columns": ["a", "b"],
            "rows": [[1]],
            "row_count": 1,
        }))
        .unwrap_err();
        assert_eq!(error.kind(), ClientErrorKind::Decode);
    }

    #[test]
    fn operation_safety_marks_execute_as_mutating_transaction() {
        let safety = operation_safety(DatabaseOperation::Execute);
        assert!(safety.mutates_state);
        assert!(safety.submits_transaction);
        assert!(safety.waits_for_receipt);
        assert!(safety.requires_owner_write_intent);
    }

    #[test]
    fn operation_safety_marks_no_wait_without_receipt_wait() {
        let safety = operation_safety(DatabaseOperation::ExecuteNoWait);
        assert!(safety.mutates_state);
        assert!(safety.submits_transaction);
        assert!(!safety.waits_for_receipt);
        assert!(safety.requires_owner_write_intent);
    }
}
