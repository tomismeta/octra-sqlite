use super::{
    error::{Error, ErrorKind, Result},
    results::ensure_receipt_success,
    rpc::{auth_info_with, compact_json, next_nonce_with, rpc_call, wait_for_receipt_with},
    safety::{Operation, OperationSafety},
    session::Session,
    transport::Transport,
};
use crate::protocol::{
    osw1,
    tx::{Tx, canonical_tx},
};
use serde_json::{Map, Value, json};
use std::env;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Unsigned owner-write transaction plus the SQL and OSW1 intent it commits to.
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
    /// Return the SQL committed to by this write.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// Return the Circle method selected for execution.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Return the Octra account nonce captured during preparation.
    pub fn nonce(&self) -> i64 {
        self.nonce
    }

    /// Return the Octra transaction timestamp captured during preparation.
    pub fn timestamp(&self) -> f64 {
        self.timestamp
    }

    /// Return the target Circle ID.
    pub fn circle(&self) -> &str {
        &self.circle
    }

    /// Return the submitting wallet address.
    pub fn wallet(&self) -> &str {
        &self.wallet
    }

    /// Return the signing public key encoded for Octra RPC.
    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    /// Return the prepared OSW1 owner-write metadata.
    pub fn owner_write(&self) -> &PreparedOwnerWrite {
        &self.owner_write
    }

    /// Return the operation's declarative safety metadata.
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

/// Database identity and sequence bound into a prepared OSW1 intent.
#[derive(Clone, PartialEq, Eq)]
pub struct PreparedOwnerWrite {
    db_id: String,
    owner_pubkey: String,
    sequence: u64,
    frame_hex: String,
}

impl PreparedOwnerWrite {
    /// Return the OSW1 database identity as hex.
    pub fn db_id(&self) -> &str {
        &self.db_id
    }

    /// Return the expected owner public key as hex.
    pub fn owner_pubkey(&self) -> &str {
        &self.owner_pubkey
    }

    /// Return the owner-write sequence committed to by the intent.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Return the complete OSW1 frame as hex.
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

/// Signed Octra transaction ready for submission.
#[derive(Clone, PartialEq)]
pub struct SignedWrite {
    tx: Tx,
    safety: OperationSafety,
}

impl SignedWrite {
    /// Borrow the signed Octra transaction.
    pub fn tx(&self) -> &Tx {
        &self.tx
    }

    /// Return the operation's declarative safety metadata.
    pub fn safety(&self) -> OperationSafety {
        self.safety
    }

    /// Consume the wrapper and return the signed transaction.
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

pub(super) fn ensure_submit_mode(signed: &SignedWrite, expected: Operation) -> Result<()> {
    let actual = signed.safety.operation;
    if actual != expected {
        return Err(Error::with_kind(
            ErrorKind::Config,
            format!("signed write was prepared for {actual:?}, not {expected:?}"),
        ));
    }
    Ok(())
}

pub(super) fn prepare_write_with<T: Transport>(
    transport: &T,
    session: &Session,
    sql: &str,
    operation: Operation,
) -> Result<PreparedWrite> {
    let nonce = next_nonce_with(transport, session)?;
    let timestamp = now_timestamp();
    let method = if trace_sql_event_enabled() {
        "exec_trace"
    } else {
        "exec"
    };
    let auth = auth_info_with(transport, session).map_err(|error| {
        error.with_context(
            "could not read Circle auth_info; refusing to choose unsigned exec implicitly",
        )
    })?;
    if !auth.configured {
        return Err(Error::with_kind(
            ErrorKind::Authorization,
            "database is not owner-write-personalized; refusing unsigned SQL write",
        ));
    }
    prepare_write_with_owner_parts(
        session,
        sql,
        operation,
        nonce,
        timestamp,
        method,
        OwnerWriteAuth {
            db_id: &auth.db_id,
            owner_pubkey: auth.owner_pubkey.as_deref(),
        },
    )
}

#[cfg(all(feature = "cli", feature = "http"))]
pub(super) fn prepare_write_with_owner_auth<T: Transport>(
    transport: &T,
    session: &Session,
    sql: &str,
    operation: Operation,
    db_id: &str,
    owner_pubkey: &str,
) -> Result<PreparedWrite> {
    let nonce = next_nonce_with(transport, session)?;
    let timestamp = now_timestamp();
    let method = if trace_sql_event_enabled() {
        "exec_trace"
    } else {
        "exec"
    };
    prepare_write_with_owner_parts(
        session,
        sql,
        operation,
        nonce,
        timestamp,
        method,
        OwnerWriteAuth {
            db_id,
            owner_pubkey: Some(owner_pubkey),
        },
    )
}

struct OwnerWriteAuth<'a> {
    db_id: &'a str,
    owner_pubkey: Option<&'a str>,
}

fn prepare_write_with_owner_parts(
    session: &Session,
    sql: &str,
    operation: Operation,
    nonce: i64,
    timestamp: f64,
    method: &str,
    auth: OwnerWriteAuth<'_>,
) -> Result<PreparedWrite> {
    let db_id_bytes = hex_to_32("db_id", auth.db_id)?;
    let session_owner_pubkey = session.intent_public_key()?;
    let owner_pubkey = match auth.owner_pubkey {
        Some(owner_pubkey) => {
            let configured = hex_to_32("owner_pubkey", owner_pubkey)?;
            if configured != session_owner_pubkey {
                return Err(Error::with_kind(
                    ErrorKind::Authorization,
                    "owner write metadata does not match the active wallet",
                ));
            }
            hex::encode(configured)
        }
        None => hex::encode(session_owner_pubkey),
    };
    let frame = osw1::frame(&db_id_bytes, nonce as u64, method, sql)?;
    let owner_write = PreparedOwnerWrite {
        db_id: auth.db_id.to_string(),
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
        public_key: session.public_key_b64()?.to_string(),
        owner_write,
        safety: operation.safety(),
    })
}

pub(super) fn sign_write(session: &Session, prepared: &PreparedWrite) -> Result<SignedWrite> {
    ensure_prepared_for_session(session, prepared)?;
    let owner_write = &prepared.owner_write;
    let params = vec![
        Value::String(prepared.sql.clone()),
        Value::String(owner_write.owner_pubkey.clone()),
        Value::String(owner_write.sequence.to_string()),
        Value::String(session.sign_owner_write_hex(&hex::decode(&owner_write.frame_hex)?)?),
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
    tx.signature = session.sign_transaction_b64(&canonical_tx(&tx))?;
    Ok(SignedWrite::new(tx, prepared.safety))
}

pub(super) fn submit_signed_write_with<T: Transport>(
    transport: &T,
    session: &Session,
    signed: SignedWrite,
    no_wait: bool,
) -> Result<Value> {
    ensure_signed_for_session(session, &signed)?;
    submit_tx_with(transport, session, signed.tx, no_wait)
}

#[cfg(any(feature = "http", test))]
pub(super) fn sign_and_submit_tx_with<T: Transport>(
    transport: &T,
    session: &Session,
    mut tx: Tx,
    no_wait: bool,
) -> Result<Value> {
    tx.signature = session.sign_transaction_b64(&canonical_tx(&tx))?;
    submit_tx_with(transport, session, tx, no_wait)
}

fn submit_tx_with<T: Transport>(
    transport: &T,
    session: &Session,
    tx: Tx,
    no_wait: bool,
) -> Result<Value> {
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
                return Err(error.with_context(format!("tx_hash: {hash}")));
            }
            out.insert("receipt".to_string(), receipt);
        }
    }
    Ok(Value::Object(out))
}

fn ensure_prepared_for_session(session: &Session, prepared: &PreparedWrite) -> Result<()> {
    if prepared.circle != session.target().circle {
        return Err(Error::with_kind(
            ErrorKind::Authorization,
            "prepared write Circle does not match the active database",
        ));
    }
    if prepared.wallet != session.caller() {
        return Err(Error::with_kind(
            ErrorKind::Authorization,
            "prepared write wallet does not match the active session",
        ));
    }
    if prepared.public_key != session.public_key_b64()? {
        return Err(Error::with_kind(
            ErrorKind::Authorization,
            "prepared write public key does not match the active session",
        ));
    }
    Ok(())
}

fn ensure_signed_for_session(session: &Session, signed: &SignedWrite) -> Result<()> {
    if signed.tx.to_ != session.target().circle {
        return Err(Error::with_kind(
            ErrorKind::Authorization,
            "signed write Circle does not match the active database",
        ));
    }
    if signed.tx.from != session.caller() {
        return Err(Error::with_kind(
            ErrorKind::Authorization,
            "signed write wallet does not match the active session",
        ));
    }
    if signed.tx.public_key != session.public_key_b64()? {
        return Err(Error::with_kind(
            ErrorKind::Authorization,
            "signed write public key does not match the active session",
        ));
    }
    Ok(())
}

fn hex_to_32(label: &str, text: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(text).map_err(|error| {
        Error::with_kind(ErrorKind::Decode, format!("decoding {label} hex: {error}"))
    })?;
    if bytes.len() != 32 {
        return Err(Error::with_kind(
            ErrorKind::Decode,
            format!("{label} must decode to 32 bytes"),
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn trace_sql_event_enabled() -> bool {
    env::var("OCTRA_SQLITE_EMIT_SQL_ONCHAIN_EVENT")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn now_timestamp() -> f64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_secs() as f64 + f64::from(duration.subsec_millis()) / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::session::{ClientOptions, build_session};
    use serde_json::Value;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct CaptureTransport {
        submits: Arc<Mutex<Vec<Value>>>,
    }

    struct AuthFailureTransport;

    impl Transport for AuthFailureTransport {
        fn call(&self, _rpc: &str, method: &str, _params: Value) -> Result<Value> {
            match method {
                "octra_balance" => Ok(json!({"pending_nonce": 41})),
                "octra_circleViewAuth" => Err(Error::with_code(
                    ErrorKind::Rpc,
                    "rpc_rate_limited",
                    "auth_info RPC was rate limited",
                )),
                _ => Err(Error::with_kind(
                    ErrorKind::Other,
                    format!("unexpected method {method}"),
                )),
            }
        }
    }

    impl Transport for CaptureTransport {
        fn call(&self, _rpc: &str, method: &str, params: Value) -> Result<Value> {
            match method {
                "octra_submit" => {
                    self.submits.lock().unwrap().push(params);
                    Ok(json!({ "tx_hash": "abc123" }))
                }
                _ => Err(Error::with_kind(
                    ErrorKind::Other,
                    format!("unexpected method {method}"),
                )),
            }
        }
    }

    fn test_session() -> Session {
        build_session(&ClientOptions {
            target: Some("oct://devnet/octABC?read_mode=sealed".to_string()),
            rpc: Some("mock://rpc".to_string()),
            caller: Some("octCaller".to_string()),
            private_key: Some(
                "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
            ),
            ..ClientOptions::default()
        })
        .unwrap()
    }

    fn tx_for(session: &Session, signature: &str) -> Tx {
        Tx {
            from: session.caller().to_string(),
            to_: session.target().circle.clone(),
            amount: "0".to_string(),
            nonce: 42,
            ou: "1000".to_string(),
            timestamp: 1000.0,
            op_type: "circle_call".to_string(),
            encrypted_data: "exec".to_string(),
            message: "[]".to_string(),
            signature: signature.to_string(),
            public_key: session.public_key_b64().unwrap().to_string(),
        }
    }

    fn submitted_signature(transport: &CaptureTransport) -> String {
        transport.submits.lock().unwrap()[0]
            .as_array()
            .and_then(|params| params.first())
            .and_then(|tx| tx.get("signature"))
            .and_then(Value::as_str)
            .unwrap()
            .to_string()
    }

    #[test]
    fn signed_write_submission_preserves_existing_signature() {
        let transport = CaptureTransport::default();
        let session = test_session();
        let tx = tx_for(&session, "pre-signed");
        let signed = SignedWrite::new(tx, Operation::ExecuteNoWait.safety());

        submit_signed_write_with(&transport, &session, signed, true).unwrap();

        assert_eq!(submitted_signature(&transport), "pre-signed");
    }

    #[test]
    fn generic_transaction_submission_signs_canonical_tx() {
        let transport = CaptureTransport::default();
        let session = test_session();
        let tx = tx_for(&session, "stale-signature");

        sign_and_submit_tx_with(&transport, &session, tx, true).unwrap();

        let signature = submitted_signature(&transport);
        assert!(!signature.is_empty());
        assert_ne!(signature, "stale-signature");
    }

    #[test]
    fn auth_preflight_preserves_source_error_code_with_context() {
        let error = prepare_write_with(
            &AuthFailureTransport,
            &test_session(),
            "create table demo(id integer);",
            Operation::Execute,
        )
        .unwrap_err();

        assert_eq!(error.kind(), ErrorKind::Rpc);
        assert_eq!(error.code(), Some("rpc_rate_limited"));
        assert!(error.to_string().contains("auth_info RPC was rate limited"));
        assert!(
            error
                .to_string()
                .contains("refusing to choose unsigned exec")
        );
    }
}
