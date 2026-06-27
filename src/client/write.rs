use super::{
    error::{ClientError, ClientErrorKind, Result},
    results::ensure_receipt_success,
    rpc::{auth_info_with, compact_json, next_nonce_with, rpc_call, wait_for_receipt_with},
    safety::{operation_safety, DatabaseOperation, OperationSafety},
    session::Session,
    transport::Transport,
};
use crate::protocol::{
    osw1,
    tx::{canonical_tx, Tx},
};
use serde_json::{json, Map, Value};
use std::env;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

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

    pub(super) fn new(tx: Tx, safety: OperationSafety) -> Self {
        Self { tx, safety }
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

pub(super) fn ensure_submit_mode(signed: &SignedWrite, expected: DatabaseOperation) -> Result<()> {
    let actual = signed.safety.operation;
    if actual != expected {
        return Err(ClientError::with_kind(
            ClientErrorKind::Config,
            format!("signed write was prepared for {actual:?}, not {expected:?}"),
        ));
    }
    Ok(())
}

pub(super) fn prepare_write_with<T: Transport>(
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

pub(super) fn sign_write(session: &Session, prepared: &PreparedWrite) -> Result<SignedWrite> {
    ensure_prepared_for_session(session, prepared)?;
    let owner_write = &prepared.owner_write;
    let params = vec![
        Value::String(prepared.sql.clone()),
        Value::String(owner_write.owner_pubkey.clone()),
        Value::String(owner_write.sequence.to_string()),
        Value::String(session.sign_owner_write_hex(&hex::decode(&owner_write.frame_hex)?)),
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
    tx.signature = session.sign_transaction_b64(&canonical_tx(&tx));
    Ok(SignedWrite::new(tx, prepared.safety))
}

pub(super) fn submit_signed_write_with<T: Transport>(
    transport: &T,
    session: &Session,
    signed: SignedWrite,
    no_wait: bool,
) -> Result<Value> {
    ensure_signed_for_session(session, &signed)?;
    submit_signed_tx_with(transport, session, signed.tx, no_wait)
}

pub(super) fn submit_signed_tx_with<T: Transport>(
    transport: &T,
    session: &Session,
    mut tx: Tx,
    no_wait: bool,
) -> Result<Value> {
    tx.signature = session.sign_transaction_b64(&canonical_tx(&tx));
    let tx_circle = tx.to_.clone();
    let tx_wallet = tx.from.clone();
    let result = rpc_call(transport, session, "octra_submit", json!([tx]))?;
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
            if let Err(error) = ensure_receipt_success(&receipt) {
                return Err(ClientError::with_kind(
                    error.kind(),
                    format!("{error}; tx_hash: {hash}"),
                ));
            }
            out.insert("receipt".to_string(), receipt);
        }
    }
    Ok(Value::Object(out))
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
