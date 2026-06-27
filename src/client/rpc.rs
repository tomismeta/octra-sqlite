use super::{
    error::{ClientError, ClientErrorKind, Result},
    results::AuthInfo,
    session::Session,
    transport::Transport,
};
use crate::protocol::osr1::{decode_typed_result, TYPED_PREFIX};
#[cfg(feature = "http")]
use crate::protocol::tx::{canonical_tx, Tx};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub(super) fn view_with<T: Transport>(
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
    let signature = session.sign_view_auth_b64(&message);
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

pub(super) fn query_typed_with<T: Transport>(
    transport: &T,
    session: &Session,
    sql: &str,
) -> Result<Value> {
    view_with(
        transport,
        session,
        "query_typed",
        vec![Value::String(sql.to_string())],
    )
}

pub(super) fn auth_info_with<T: Transport>(transport: &T, session: &Session) -> Result<AuthInfo> {
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

pub(super) fn program_info_with<T: Transport>(transport: &T, session: &Session) -> Result<Value> {
    let message = format!(
        "octra_circle_program_info|{}|{}",
        session.target().circle,
        session.caller()
    );
    let signature = session.sign_program_info_b64(&message);
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

pub(super) fn next_nonce_with<T: Transport>(transport: &T, session: &Session) -> Result<i64> {
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

pub(super) fn rpc_call<T: Transport>(
    transport: &T,
    session: &Session,
    method: &str,
    params: Value,
) -> Result<Value> {
    transport.call(session.rpc(), method, params)
}

pub(super) fn wait_for_receipt_with<T: Transport>(
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

#[cfg(feature = "http")]
pub(super) fn wait_for_transaction_with<T: Transport>(
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

pub(super) fn compact_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
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
    let value = serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()));
    if let Some(error) = contract_error_text(&value) {
        return Err(ClientError::with_kind(ClientErrorKind::Rpc, error));
    }
    Ok(value)
}

fn contract_error_text(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    let failed = object.get("ok").and_then(Value::as_bool) == Some(false);
    let code = object.get("error").and_then(Value::as_str);
    if !failed && code.is_none() {
        return None;
    }
    let code = code.unwrap_or("contract_error");
    match object.get("detail").and_then(Value::as_str) {
        Some(detail) if !detail.is_empty() => Some(format!("database error ({code}): {detail}")),
        _ => Some(format!("database error ({code})")),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(feature = "http")]
pub(super) fn sign_canonical_tx(session: &Session, tx: &mut Tx) -> Result<()> {
    let canonical = canonical_tx(tx);
    tx.signature = session.sign_transaction_b64(&canonical);
    Ok(())
}
