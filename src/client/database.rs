use super::session::{build_session, Session, SessionOptions};
use crate::protocol::{
    osr1::{decode_typed_result, TYPED_PREFIX},
    osw1,
    tx::{canonical_tx, Tx},
};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Default)]
pub struct OctraSqlite {
    options: SessionOptions,
}

impl OctraSqlite {
    pub fn from_default_config() -> Result<Self> {
        super::config::load_config()?;
        Ok(Self::default())
    }

    pub fn with_options(options: SessionOptions) -> Self {
        Self { options }
    }

    pub fn database(&self, target: impl Into<String>) -> Result<Database> {
        let mut options = self.options.clone();
        options.target = Some(target.into());
        Database::open(options)
    }
}

#[derive(Clone)]
pub struct Database {
    session: Session,
}

impl Database {
    pub fn open(options: SessionOptions) -> Result<Self> {
        Ok(Self {
            session: build_session(&options)?,
        })
    }

    pub fn query(&self, sql: &str) -> Result<Value> {
        query_typed(&self.session, sql)
    }

    pub fn execute(&self, sql: &str) -> Result<Value> {
        exec_sql(&self.session, sql, false)
    }

    pub fn execute_no_wait(&self, sql: &str) -> Result<Value> {
        exec_sql(&self.session, sql, true)
    }

    pub fn auth_info(&self) -> Result<AuthInfo> {
        auth_info(&self.session)
    }

    pub fn program_info(&self) -> Result<Value> {
        program_info(&self.session)
    }
}

pub struct AuthInfo {
    pub configured: bool,
    pub db_id: String,
    pub owner_pubkey: Option<String>,
    pub owner_sequence: Option<u64>,
}

pub fn view(session: &Session, method: &str, params: Vec<Value>) -> Result<Value> {
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

pub fn query_typed(session: &Session, sql: &str) -> Result<Value> {
    view(session, "query_typed", vec![Value::String(sql.to_string())])
}

pub fn auth_info(session: &Session) -> Result<AuthInfo> {
    let value = view(session, "auth_info", vec![])?;
    let configured = value
        .get("configured")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let db_id = value
        .get("db_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("auth_info missing db_id"))?
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

pub fn program_info(session: &Session) -> Result<Value> {
    let message = format!(
        "octra_circle_program_info|{}|{}",
        session.target().circle,
        session.caller()
    );
    let signature = session.sign_text_b64(&message)?;
    rpc_call(
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

pub fn exec_sql(session: &Session, sql: &str, no_wait: bool) -> Result<Value> {
    let nonce = next_nonce(session)?;
    let timestamp = now_timestamp();
    let method = if trace_sql_event_enabled() {
        "exec_trace"
    } else {
        "exec"
    };
    let auth = auth_info(session).with_context(|| {
        "could not read Circle auth_info; refusing to choose unsigned exec implicitly"
    })?;
    let params = if auth.configured {
        signed_exec_params(session, &auth, nonce as u64, method, sql)?
    } else {
        vec![Value::String(sql.to_string())]
    };
    let message = compact_json(&Value::Array(params))?;
    let tx = Tx {
        from: session.caller().to_string(),
        to_: session.target().circle.clone(),
        amount: "0".to_string(),
        nonce,
        ou: "1000".to_string(),
        timestamp,
        op_type: "circle_call".to_string(),
        encrypted_data: method.to_string(),
        message,
        signature: String::new(),
        public_key: session.public_key_b64().to_string(),
    };
    submit_tx(session, tx, no_wait)
}

pub fn next_nonce(session: &Session) -> Result<i64> {
    let balance = rpc_call(session, "octra_balance", json!([session.caller()]))?;
    Ok(balance
        .get("pending_nonce")
        .or_else(|| balance.get("nonce"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        + 1)
}

pub fn submit_tx(session: &Session, mut tx: Tx, no_wait: bool) -> Result<Value> {
    let canonical = canonical_tx(&tx);
    tx.signature = session.sign_text_b64(&canonical)?;
    let tx_circle = tx.to_.clone();
    let tx_wallet = tx.from.clone();
    let result = rpc_call(session, "octra_submit", json!([tx]))?;
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
            let receipt = wait_for_receipt(session, &hash)?;
            out.insert("receipt".to_string(), receipt);
        }
    }
    Ok(Value::Object(out))
}

pub fn wait_for_transaction(session: &Session, tx_hash: &str) -> Result<Value> {
    for _ in 0..60 {
        let result = rpc_call(session, "octra_transaction", json!([tx_hash]));
        if let Ok(transaction) = result {
            let status = transaction
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match status {
                "confirmed" | "accepted" => return Ok(transaction),
                "rejected" | "failed" => bail!("transaction {tx_hash} {status}: {transaction}"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    bail!("timed out waiting for transaction {tx_hash}")
}

fn rpc_call(session: &Session, method: &str, params: Value) -> Result<Value> {
    let client = reqwest::blocking::Client::new();
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });
    let response = client
        .post(session.rpc())
        .json(&body)
        .send()
        .with_context(|| format!("calling {method}"))?;
    let status = response.status();
    let payload: Value = response
        .json()
        .with_context(|| format!("decoding {method} response"))?;
    if !status.is_success() {
        bail!("{method} failed with HTTP {status}: {payload}");
    }
    if let Some(error) = payload.get("error") {
        bail!("{method} failed: {error}");
    }
    Ok(payload.get("result").cloned().unwrap_or(Value::Null))
}

fn signed_exec_params(
    session: &Session,
    info: &AuthInfo,
    sequence: u64,
    method: &str,
    sql: &str,
) -> Result<Vec<Value>> {
    let db_id = hex_to_32("db_id", &info.db_id)?;
    let pubkey_hex = hex::encode(session.intent_public_key()?);
    let sequence_text = sequence.to_string();
    let message = osw1::frame(&db_id, sequence, method, sql)?;
    let sig_hex = session.sign_bytes_hex(&message)?;
    Ok(vec![
        Value::String(sql.to_string()),
        Value::String(pubkey_hex),
        Value::String(sequence_text),
        Value::String(sig_hex),
    ])
}

fn wait_for_receipt(session: &Session, tx_hash: &str) -> Result<Value> {
    for _ in 0..45 {
        let result = rpc_call(session, "contract_receipt", json!([tx_hash]));
        if let Ok(receipt) = result {
            if !receipt.is_null() {
                return Ok(receipt);
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    bail!("timed out waiting for receipt {tx_hash}")
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
        return decode_typed_result(encoded);
    }
    Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string())))
}

fn compact_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn hex_to_32(label: &str, text: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(text).with_context(|| format!("decoding {label} hex"))?;
    if bytes.len() != 32 {
        bail!("{label} must decode to 32 bytes");
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
