use super::{
    error::{Error, ErrorKind, Result},
    results::AuthInfo,
    session::Session,
    transport::Transport,
};
use crate::protocol::osr1::{TYPED_PREFIX, decode_typed_result};
use crate::protocol::target::ReadMode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub(super) fn view_with<T: Transport>(
    transport: &T,
    session: &Session,
    method: &str,
    params: Vec<Value>,
) -> Result<Value> {
    match session.target().read_mode {
        ReadMode::Public => return public_view_with(transport, session, method, params),
        ReadMode::Auto if circle_is_public_read(&circle_info_with(transport, session)?) => {
            return public_view_with(transport, session, method, params);
        }
        _ => {}
    }
    signed_view_with(transport, session, method, params)
}

fn signed_view_with<T: Transport>(
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
    let signature = session.sign_view_auth_b64(&message)?;
    let result = rpc_call(
        transport,
        session,
        "octra_circleViewAuth",
        json!([
            session.target().circle,
            method,
            params,
            session.caller(),
            session.public_key_b64()?,
            signature,
            false
        ]),
    )?;
    decode_rpc_result(result)
}

fn public_view_with<T: Transport>(
    transport: &T,
    session: &Session,
    method: &str,
    params: Vec<Value>,
) -> Result<Value> {
    let result = rpc_call(
        transport,
        session,
        "octra_circleView",
        json!([
            session.target().circle,
            method,
            params,
            session.caller(),
            false
        ]),
    )?;
    decode_rpc_result(result)
}

pub(super) fn circle_info_with<T: Transport>(transport: &T, session: &Session) -> Result<Value> {
    rpc_call(
        transport,
        session,
        "octra_circleInfo",
        json!([session.target().circle]),
    )
}

fn circle_is_public_read(info: &Value) -> bool {
    info.get("privacy_class").and_then(Value::as_str) == Some("public")
        && info.get("browser_mode").and_then(Value::as_str) == Some("gateway_allowed")
        && info.get("resource_mode").and_then(Value::as_str) == Some("public_resources")
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
        .ok_or_else(|| Error::with_kind(ErrorKind::Decode, "auth_info missing db_id"))?
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
    match session.target().read_mode {
        ReadMode::Public => return public_program_info_with(transport, session),
        ReadMode::Auto if circle_is_public_read(&circle_info_with(transport, session)?) => {
            return public_program_info_with(transport, session);
        }
        _ => {}
    }
    let message = format!(
        "octra_circle_program_info|{}|{}",
        session.target().circle,
        session.caller()
    );
    let signature = session.sign_program_info_b64(&message)?;
    rpc_call(
        transport,
        session,
        "octra_circleProgramInfoAuth",
        json!([
            session.target().circle,
            session.caller(),
            session.public_key_b64()?,
            signature
        ]),
    )
}

fn public_program_info_with<T: Transport>(transport: &T, session: &Session) -> Result<Value> {
    rpc_call(
        transport,
        session,
        "octra_circleProgramInfo",
        json!([session.target().circle]),
    )
}

pub(super) fn next_nonce_with<T: Transport>(transport: &T, session: &Session) -> Result<i64> {
    let balance = rpc_call(
        transport,
        session,
        "octra_balance",
        json!([session.caller()]),
    )?;
    next_nonce_from_balance(&balance)
}

fn next_nonce_from_balance(balance: &Value) -> Result<i64> {
    let value = balance
        .get("pending_nonce")
        .or_else(|| balance.get("nonce"))
        .ok_or_else(|| Error::with_kind(ErrorKind::Decode, "balance response missing nonce"))?;
    let nonce = value
        .as_i64()
        .or_else(|| value.as_str()?.parse::<i64>().ok())
        .filter(|nonce| *nonce >= 0)
        .ok_or_else(|| {
            Error::with_kind(
                ErrorKind::Decode,
                "balance response nonce must be a non-negative integer",
            )
        })?;
    nonce
        .checked_add(1)
        .ok_or_else(|| Error::with_kind(ErrorKind::Decode, "balance response nonce exceeds i64"))
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
        if let Ok(receipt) = result
            && !receipt.is_null()
        {
            return Ok(receipt);
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err(Error::with_kind(
        ErrorKind::Timeout,
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
                    return Err(Error::with_kind(
                        ErrorKind::Receipt,
                        format!("transaction {tx_hash} {status}: {transaction}"),
                    ));
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err(Error::with_kind(
        ErrorKind::Timeout,
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
    if let Some((code, error)) = contract_error(&value) {
        let kind = if code.starts_with("auth_") {
            ErrorKind::Authorization
        } else {
            ErrorKind::Rpc
        };
        return Err(Error::with_code(kind, code, error));
    }
    Ok(value)
}

fn contract_error(value: &Value) -> Option<(&str, String)> {
    let object = value.as_object()?;
    let failed = object.get("ok").and_then(Value::as_bool) == Some(false);
    let code = object.get("error").and_then(Value::as_str);
    if !failed && code.is_none() {
        return None;
    }
    let code = code.unwrap_or("contract_error");
    let message = match object.get("detail").and_then(Value::as_str) {
        Some(detail) if !detail.is_empty() => format!("database error ({code}): {detail}"),
        _ => format!("database error ({code})"),
    };
    Some((code, message))
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_parsing_accepts_numbers_and_decimal_strings() {
        assert_eq!(
            next_nonce_from_balance(&json!({"pending_nonce": 41})).unwrap(),
            42
        );
        assert_eq!(next_nonce_from_balance(&json!({"nonce": "9"})).unwrap(), 10);
    }

    #[test]
    fn nonce_parsing_fails_closed() {
        for value in [
            json!({}),
            json!({"nonce": null}),
            json!({"nonce": "nope"}),
            json!({"nonce": -1}),
            json!({"nonce": i64::MAX}),
        ] {
            assert!(next_nonce_from_balance(&value).is_err(), "{value}");
        }
    }

    #[test]
    fn contract_auth_errors_keep_authorization_kind_and_source_code() {
        let error = decode_method_result(
            r#"{"ok":false,"error":"auth_denied","detail":"signer is not the owner"}"#,
        )
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Authorization);
        assert_eq!(error.code(), Some("auth_denied"));
    }
}
