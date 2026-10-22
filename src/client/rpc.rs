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
        ReadMode::Auto
            if circle_info_allows_unsigned_read(&circle_info_with(transport, session)?) =>
        {
            return public_view_with(transport, session, method, params);
        }
        _ => {}
    }
    signed_view_with(transport, session, method, params)
}

fn circle_auth_message(op: &str, circle_id: &str, caller: &str, subject: &str) -> String {
    format!(
        "octra_circle_auth_v2|{}:{}|{}:{}|{}:{}|{}:{}",
        op.len(),
        op,
        circle_id.len(),
        circle_id,
        caller.len(),
        caller,
        subject.len(),
        subject
    )
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
    let message = circle_auth_message(
        "octra_circle_view",
        &session.target().circle,
        session.caller(),
        &format!("{method}|{params_hash}|0"),
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

pub(crate) fn circle_info_allows_unsigned_read(info: &Value) -> bool {
    info.get("privacy_class").and_then(Value::as_str) == Some("public")
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
        ReadMode::Auto
            if circle_info_allows_unsigned_read(&circle_info_with(transport, session)?) =>
        {
            return public_program_info_with(transport, session);
        }
        _ => {}
    }
    let message = circle_auth_message(
        "octra_circle_program_info",
        &session.target().circle,
        session.caller(),
        "",
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
    wait_for_receipt_with_policy(transport, session, tx_hash, 45, Duration::from_secs(2))
}

fn wait_for_receipt_with_policy<T: Transport>(
    transport: &T,
    session: &Session,
    tx_hash: &str,
    attempts: usize,
    delay: Duration,
) -> Result<Value> {
    let mut last_error = None;
    for attempt in 0..attempts {
        let result = rpc_call(transport, session, "contract_receipt", json!([tx_hash]));
        match result {
            Ok(receipt) if !receipt.is_null() => return Ok(receipt),
            Ok(_) => last_error = None,
            Err(error) if rpc_not_found(&error) => last_error = None,
            Err(error) => last_error = Some(error),
        }
        if attempt + 1 < attempts && !delay.is_zero() {
            std::thread::sleep(delay);
        }
    }
    // A rejected or dropped transaction may never have a contract receipt.
    // Consult its disposition once, without doubling every receipt poll.
    match rpc_call(transport, session, "octra_transaction", json!([tx_hash])) {
        Ok(transaction) => {
            if let Some(error) = terminal_transaction_error(tx_hash, transaction) {
                return Err(error);
            }
        }
        Err(error) if rpc_not_found(&error) => {}
        Err(error) => {
            if last_error.is_none() {
                last_error = Some(error);
            }
        }
    }
    if let Some(error) = last_error {
        return Err(error.with_context(format!("waiting for receipt {tx_hash}")));
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
    wait_for_transaction_with_policy(transport, session, tx_hash, 60, Duration::from_secs(2))
}

#[cfg(any(feature = "http", test))]
fn wait_for_transaction_with_policy<T: Transport>(
    transport: &T,
    session: &Session,
    tx_hash: &str,
    attempts: usize,
    delay: Duration,
) -> Result<Value> {
    let mut last_error = None;
    for attempt in 0..attempts {
        match rpc_call(transport, session, "octra_transaction", json!([tx_hash])) {
            Ok(transaction) => {
                if matches!(
                    transaction.get("status").and_then(Value::as_str),
                    Some("confirmed" | "accepted")
                ) {
                    return Ok(transaction);
                }
                if let Some(error) = terminal_transaction_error(tx_hash, transaction) {
                    return Err(error);
                }
                last_error = None;
            }
            Err(error) if rpc_not_found(&error) => last_error = None,
            Err(error) => last_error = Some(error),
        }
        if attempt + 1 < attempts && !delay.is_zero() {
            std::thread::sleep(delay);
        }
    }
    if let Some(error) = last_error {
        return Err(error.with_context(format!("waiting for transaction {tx_hash}")));
    }
    Err(Error::with_kind(
        ErrorKind::Timeout,
        format!("timed out waiting for transaction {tx_hash}"),
    ))
}

fn rpc_not_found(error: &Error) -> bool {
    error.kind() == ErrorKind::Rpc && error.code() == Some("rpc_not_found")
}

fn terminal_transaction_error(tx_hash: &str, transaction: Value) -> Option<Error> {
    let status = transaction.get("status").and_then(Value::as_str)?;
    let code = match status {
        "rejected" => "transaction_rejected",
        "failed" => "transaction_failed",
        "dropped" => "transaction_dropped",
        _ => return None,
    };
    Some(Error::with_code_and_details(
        ErrorKind::Receipt,
        code,
        format!("transaction {tx_hash} {status}: {transaction}"),
        [
            ("tx_hash", Value::String(tx_hash.to_string())),
            ("status", Value::String(status.to_string())),
            ("transaction", transaction),
        ],
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
    use crate::client::session::{ClientOptions, build_session};
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signature, VerifyingKey};

    struct V2Transport;

    impl Transport for V2Transport {
        fn call(&self, _rpc: &str, method: &str, params: Value) -> Result<Value> {
            let (domain, circle, caller, subject, key, signature) = match method {
                "octra_circleViewAuth" => (
                    "octra_circle_view",
                    params[0].as_str().unwrap(),
                    params[3].as_str().unwrap(),
                    format!(
                        "{}|{}|0",
                        params[1].as_str().unwrap(),
                        hex::encode(Sha256::digest(serde_json::to_vec(&params[2]).unwrap()))
                    ),
                    params[4].as_str().unwrap(),
                    params[5].as_str().unwrap(),
                ),
                "octra_circleProgramInfoAuth" => (
                    "octra_circle_program_info",
                    params[0].as_str().unwrap(),
                    params[1].as_str().unwrap(),
                    String::new(),
                    params[2].as_str().unwrap(),
                    params[3].as_str().unwrap(),
                ),
                _ => panic!("unexpected RPC {method}"),
            };
            let key = VerifyingKey::from_bytes(&STANDARD.decode(key).unwrap().try_into().unwrap())
                .unwrap();
            let signature = Signature::from_slice(&STANDARD.decode(signature).unwrap()).unwrap();
            // Independent upstream frame implementation: lengths count encoded bytes.
            let frame = |parts: &[&str]| {
                let mut bytes = b"octra_circle_auth_v2".to_vec();
                for part in parts {
                    bytes.extend_from_slice(format!("|{}:", part.len()).as_bytes());
                    bytes.extend_from_slice(part.as_bytes());
                }
                bytes
            };
            key.verify_strict(&frame(&[domain, circle, caller, &subject]), &signature)
                .map_err(|_| Error::with_kind(ErrorKind::Authorization, "invalid v2 signature"))?;
            for changed in [
                frame(&["wrong_domain", circle, caller, &subject]),
                frame(&[domain, "another_circle", caller, &subject]),
                frame(&[domain, circle, "another_caller", &subject]),
                frame(&[domain, circle, caller, &format!("wrong_method|{subject}")]),
                frame(&[domain, circle, caller, &format!("{subject}tampered_params")]),
            ] {
                assert!(key.verify_strict(&changed, &signature).is_err());
            }
            Ok(json!({"verified": true}))
        }
    }

    #[test]
    fn signed_reads_verify_against_upstream_v2_frames() {
        let session = test_session();
        for sql in ["select 42;", "select 'héllo ✓';"] {
            assert_eq!(
                signed_view_with(&V2Transport, &session, "query", vec![json!(sql)]).unwrap(),
                json!({"verified": true})
            );
        }
        assert_eq!(
            program_info_with(&V2Transport, &session).unwrap(),
            json!({"verified": true})
        );
    }

    #[test]
    fn v2_frames_preserve_utf8_delimiters_and_empty_subjects() {
        assert_eq!(
            circle_auth_message("op", "圈", "a|b", "hé|✓:"),
            "octra_circle_auth_v2|2:op|3:圈|3:a|b|8:hé|✓:"
        );
        assert_eq!(
            circle_auth_message("op", "c", "a", ""),
            "octra_circle_auth_v2|2:op|1:c|1:a|0:"
        );
    }

    struct OutcomeTransport {
        responses: std::cell::RefCell<std::collections::VecDeque<(&'static str, Result<Value>)>>,
    }

    impl OutcomeTransport {
        fn new(responses: impl IntoIterator<Item = (&'static str, Result<Value>)>) -> Self {
            Self {
                responses: std::cell::RefCell::new(responses.into_iter().collect()),
            }
        }

        fn exhausted(&self) {
            assert!(self.responses.borrow().is_empty());
        }
    }

    impl Transport for OutcomeTransport {
        fn call(&self, _rpc: &str, method: &str, params: Value) -> Result<Value> {
            let (expected, result) = self.responses.borrow_mut().pop_front().unwrap();
            assert_eq!(method, expected);
            assert_eq!(params, json!(["abc123"]));
            result
        }
    }

    fn missing_outcome() -> Result<Value> {
        Err(Error::with_code(
            ErrorKind::Rpc,
            "rpc_not_found",
            "requested outcome not found",
        ))
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

    #[test]
    fn receipt_wait_resolves_terminal_dispositions_once_after_exhaustion() {
        for status in ["rejected", "failed", "dropped"] {
            let transaction = json!({
                "status": status,
                "error": {"reason": "insufficient budget"},
                "detail": {"required": 1234}
            });
            let transport = OutcomeTransport::new([
                ("contract_receipt", missing_outcome()),
                ("contract_receipt", Ok(Value::Null)),
                ("contract_receipt", missing_outcome()),
                ("octra_transaction", Ok(transaction.clone())),
            ]);
            let error = wait_for_receipt_with_policy(
                &transport,
                &test_session(),
                "abc123",
                3,
                Duration::ZERO,
            )
            .unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Receipt);
            assert_eq!(error.code(), Some(format!("transaction_{status}").as_str()));
            let details = error.details().unwrap();
            assert_eq!(details["tx_hash"], "abc123");
            assert_eq!(details["status"], status);
            assert_eq!(details["transaction"], transaction);
            assert!(error.to_string().contains("insufficient budget"));
            transport.exhausted();
        }
    }

    #[test]
    fn receipt_wait_preserves_receipts_without_transaction_lookup() {
        for receipt in [
            json!({"success": true, "contract": "octABC"}),
            json!({"success": false, "error": "sqlite_exec_failed"}),
        ] {
            let transport = OutcomeTransport::new([
                ("contract_receipt", missing_outcome()),
                ("contract_receipt", Ok(receipt.clone())),
            ]);
            assert_eq!(
                wait_for_receipt_with_policy(
                    &transport,
                    &test_session(),
                    "abc123",
                    3,
                    Duration::ZERO,
                )
                .unwrap(),
                receipt
            );
            transport.exhausted();
        }
    }

    #[test]
    fn receipt_wait_keeps_nonterminal_and_unknown_dispositions_unresolved() {
        for transaction in [
            Ok(json!({"status": "pending"})),
            Ok(json!({"status": "accepted"})),
            Ok(json!({"status": "confirmed"})),
            Ok(json!({"status": "unknown"})),
            Ok(Value::Null),
            missing_outcome(),
        ] {
            let transport = OutcomeTransport::new([
                ("contract_receipt", missing_outcome()),
                ("contract_receipt", Ok(Value::Null)),
                ("octra_transaction", transaction),
            ]);
            let error = wait_for_receipt_with_policy(
                &transport,
                &test_session(),
                "abc123",
                2,
                Duration::ZERO,
            )
            .unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Timeout);
            assert!(error.to_string().contains("abc123"));
            transport.exhausted();
        }
    }

    #[test]
    fn receipt_wait_does_not_mask_rpc_or_transport_errors_as_pending() {
        for kind in [
            ErrorKind::Rpc,
            ErrorKind::Transport,
            ErrorKind::Authorization,
        ] {
            let source = Error::with_code(kind, "source_failure", "unavailable");
            for responses in [
                vec![
                    ("contract_receipt", Ok(Value::Null)),
                    ("contract_receipt", Err(source.clone())),
                    ("octra_transaction", Ok(json!({"status": "pending"}))),
                ],
                vec![
                    ("contract_receipt", missing_outcome()),
                    ("contract_receipt", Ok(Value::Null)),
                    ("octra_transaction", Err(source.clone())),
                ],
            ] {
                let transport = OutcomeTransport::new(responses);
                let error = wait_for_receipt_with_policy(
                    &transport,
                    &test_session(),
                    "abc123",
                    2,
                    Duration::ZERO,
                )
                .unwrap_err();
                assert_eq!(error.kind(), kind);
                assert_eq!(error.code(), source.code());
                assert!(error.to_string().contains("waiting for receipt abc123"));
                transport.exhausted();
            }
        }
    }

    #[test]
    fn transaction_wait_stops_on_terminal_failure_after_pending() {
        for status in ["rejected", "failed", "dropped"] {
            let transaction = json!({
                "status": status, "reason": "expired", "detail": "removed from staging"
            });
            let transport = OutcomeTransport::new([
                ("octra_transaction", Ok(json!({"status": "pending"}))),
                ("octra_transaction", Ok(transaction.clone())),
            ]);
            let error = wait_for_transaction_with_policy(
                &transport,
                &test_session(),
                "abc123",
                60,
                Duration::ZERO,
            )
            .unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Receipt);
            assert_eq!(error.details().unwrap()["transaction"], transaction);
            assert_eq!(error.details().unwrap()["tx_hash"], "abc123");
            transport.exhausted();
        }
    }

    #[test]
    fn transaction_wait_preserves_success_and_distinguishes_unknown_from_rpc_failure() {
        for status in ["accepted", "confirmed"] {
            let transaction = json!({"status": status});
            let transport = OutcomeTransport::new([
                ("octra_transaction", missing_outcome()),
                ("octra_transaction", Ok(transaction.clone())),
            ]);
            assert_eq!(
                wait_for_transaction_with_policy(
                    &transport,
                    &test_session(),
                    "abc123",
                    2,
                    Duration::ZERO,
                )
                .unwrap(),
                transaction
            );
            transport.exhausted();
        }
        for (response, expected_kind) in [
            (missing_outcome(), ErrorKind::Timeout),
            (Ok(json!({"status": "pending"})), ErrorKind::Timeout),
            (
                Err(Error::with_kind(ErrorKind::Transport, "connection lost")),
                ErrorKind::Transport,
            ),
        ] {
            let transport = OutcomeTransport::new([("octra_transaction", response)]);
            let error = wait_for_transaction_with_policy(
                &transport,
                &test_session(),
                "abc123",
                1,
                Duration::ZERO,
            )
            .unwrap_err();
            assert_eq!(error.kind(), expected_kind);
            transport.exhausted();
        }
    }
}
