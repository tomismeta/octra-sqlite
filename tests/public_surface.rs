// This test is an ontology tripwire. The imports are the assertion.
#![allow(unused_imports)]

use octra_sqlite::{
    AuthInfo, Client, ClientOptions, Database, Error, ErrorKind, ExecuteResult, ProgramInfo,
    QueryResult, ReadMode, Result, StorageInfo, SubmittedTransaction, Value,
};

use octra_sqlite::client::{
    Config, DatabaseMetadata, NetworkConfig, Operation, OperationSafety, PreparedOwnerWrite,
    PreparedWrite, SignedWrite, Transport, config_path, load_config, write_config,
};

#[cfg(feature = "http")]
use octra_sqlite::client::{HttpTransport, RpcTraceMode};

use octra_sqlite::client::raw::{
    ClientOptions as RawClientOptions, Session, build_control_session, build_session,
    discover_wallet_path, resolve_database_target, resolve_wallet_path, wallet_caller,
};

#[cfg(feature = "http")]
use octra_sqlite::client::raw::{
    auth_info, circle_info, exec_sql, next_nonce, program_info, query_typed, query_typed_traced,
    submit_tx, view, wait_for_transaction,
};

use octra_sqlite::protocol::{error, osr1, osw1, target, tx};

#[cfg(feature = "cli")]
use octra_sqlite::cli::{error_code as cli_error_code, run, run_with_exit_code};

#[test]
fn public_surface_imports_compile() {
    assert_eq!(ReadMode::Public.as_str(), "public");
    assert!(Operation::Execute.safety().submits_transaction);
    assert_eq!(Error::new("surface").code(), None);
    let _: fn(&ExecuteResult) -> Option<u64> = ExecuteResult::effort;
    #[cfg(feature = "http")]
    {
        let _: Client = Client::default();
        let _: fn(&Database) -> Result<StorageInfo> = Database::storage_info;
    }
}

// These consumer tests deliberately require neither the CLI nor HTTP features.
mod typed_write_budgets {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use ed25519_dalek::{Signature, SigningKey};
    use serde_json::{Value as JsonValue, json};
    use std::sync::{Arc, Mutex};

    const SQL: &str = "insert into demo(body) values ('héllo ✓');";

    #[derive(Clone, Default)]
    struct BudgetTransport {
        submitted: Arc<Mutex<Vec<JsonValue>>>,
        receipts: Arc<Mutex<usize>>,
    }

    // Assemble the upstream transaction field order independently of canonical_tx.
    fn signing_bytes(wire: &JsonValue) -> Vec<u8> {
        let fields = [
            "from",
            "to_",
            "amount",
            "nonce",
            "ou",
            "timestamp",
            "op_type",
            "encrypted_data",
            "message",
        ];
        format!(
            "{{{}}}",
            fields
                .iter()
                .map(|field| format!("\"{field}\":{}", wire[field]))
                .collect::<Vec<_>>()
                .join(",")
        )
        .into_bytes()
    }

    fn assert_budget_is_signed(wire: &JsonValue) {
        let key = SigningKey::from_bytes(&[1; 32]).verifying_key();
        assert_eq!(wire["public_key"], BASE64.encode(key.as_bytes()));
        let signature =
            Signature::from_slice(&BASE64.decode(wire["signature"].as_str().unwrap()).unwrap())
                .unwrap();
        key.verify_strict(&signing_bytes(wire), &signature).unwrap();

        let mut tampered = wire.clone();
        tampered["ou"] = json!(if wire["ou"] == "1" { "2" } else { "1" });
        assert!(
            key.verify_strict(&signing_bytes(&tampered), &signature)
                .is_err()
        );
        let params: JsonValue = serde_json::from_str(wire["message"].as_str().unwrap()).unwrap();
        assert_eq!(params[0], SQL);
    }

    impl Transport for BudgetTransport {
        fn call(&self, _rpc: &str, method: &str, params: JsonValue) -> Result<JsonValue> {
            match method {
                "octra_balance" => Ok(json!({ "pending_nonce": 41 })),
                "octra_circleViewAuth" => {
                    assert_eq!(params[1], "auth_info");
                    Ok(json!({
                        "configured": true,
                        "db_id": "1111111111111111111111111111111111111111111111111111111111111111",
                    }))
                }
                "octra_submit" => {
                    assert_budget_is_signed(&params[0]);
                    self.submitted.lock().unwrap().push(params[0].clone());
                    Ok(json!({ "tx_hash": "budget-tx" }))
                }
                "contract_receipt" => {
                    *self.receipts.lock().unwrap() += 1;
                    Ok(json!({ "success": true, "error": null, "method": "exec" }))
                }
                _ => panic!("unexpected RPC method {method}"),
            }
        }
    }

    fn database() -> (Database<BudgetTransport>, BudgetTransport) {
        let transport = BudgetTransport::default();
        let client = Client::with_transport(
            ClientOptions {
                rpc: Some("mock://rpc".to_string()),
                caller: Some("octCaller".to_string()),
                private_key: Some(
                    "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
                ),
                ..ClientOptions::default()
            },
            transport.clone(),
        );
        let db = client
            .database("oct://devnet/octABC?read_mode=sealed")
            .unwrap();
        (db, transport)
    }

    #[test]
    fn explicit_budgets_reach_signed_typed_submissions_in_all_modes() {
        // The upper bound is the existing client's u64 validation limit, not
        // a claim that any particular node accepts that execution budget.
        for (input, expected) in [
            ("1", "1"),
            ("18446744073709551615", "18446744073709551615"),
            (" 050000 ", "050000"),
        ] {
            for mode in 0..4 {
                let (db, transport) = database();
                let submitted = match mode {
                    0 => db.execute_with_ou(SQL, input).unwrap().submitted,
                    1 => db.execute_no_wait_with_ou(SQL, input).unwrap(),
                    _ => {
                        let prepared = if mode == 2 {
                            db.prepare_write_with_ou(SQL, input).unwrap()
                        } else {
                            db.prepare_write_no_wait_with_ou(SQL, input).unwrap()
                        };
                        assert_eq!(prepared.ou(), expected);
                        assert_eq!(prepared.sql(), SQL);
                        assert!(transport.submitted.lock().unwrap().is_empty());
                        let signed = db.sign_write(&prepared).unwrap();
                        assert_budget_is_signed(&serde_json::to_value(signed.tx()).unwrap());
                        assert!(transport.submitted.lock().unwrap().is_empty());
                        // Explicit OU must not erase the reviewed submission mode.
                        let wrong_mode = if mode == 2 {
                            db.submit_signed_write(signed.clone()).unwrap_err()
                        } else {
                            db.submit_signed_write_and_wait(signed.clone()).unwrap_err()
                        };
                        assert_eq!(wrong_mode.kind(), ErrorKind::Config);
                        assert!(transport.submitted.lock().unwrap().is_empty());
                        if mode == 2 {
                            db.submit_signed_write_and_wait(signed).unwrap().submitted
                        } else {
                            db.submit_signed_write(signed).unwrap()
                        }
                    }
                };
                assert_eq!(submitted.tx_hash.as_deref(), Some("budget-tx"));
                let sent = transport.submitted.lock().unwrap();
                assert_eq!(sent.len(), 1);
                assert_eq!(sent[0]["ou"], expected);
                assert_eq!(sent[0]["nonce"], 42);
                assert_eq!(
                    *transport.receipts.lock().unwrap(),
                    usize::from(mode % 2 == 0)
                );
            }
        }
    }

    #[test]
    fn invalid_explicit_budgets_never_submit_in_any_typed_mode() {
        let (db, transport) = database();
        for ou in [
            "",
            " ",
            "0",
            "-1",
            "+1",
            "1.5",
            "abc",
            "18446744073709551616",
        ] {
            let errors = [
                db.execute_with_ou(SQL, ou).unwrap_err(),
                db.execute_no_wait_with_ou(SQL, ou).unwrap_err(),
                db.prepare_write_with_ou(SQL, ou).unwrap_err(),
                db.prepare_write_no_wait_with_ou(SQL, ou).unwrap_err(),
            ];
            for error in errors {
                assert_eq!(error.kind(), ErrorKind::Config, "OU: {ou:?}");
            }
        }
        assert!(transport.submitted.lock().unwrap().is_empty());
        assert_eq!(*transport.receipts.lock().unwrap(), 0);
    }

    #[test]
    fn default_typed_operations_keep_the_existing_signed_budget() {
        let (db, transport) = database();
        db.execute(SQL).unwrap();
        db.execute_no_wait(SQL).unwrap();
        let confirmed = db.prepare_write(SQL).unwrap();
        let no_wait = db.prepare_write_no_wait(SQL).unwrap();
        assert_eq!(confirmed.ou(), "1000");
        assert_eq!(no_wait.ou(), "1000");
        db.submit_signed_write_and_wait(db.sign_write(&confirmed).unwrap())
            .unwrap();
        db.submit_signed_write(db.sign_write(&no_wait).unwrap())
            .unwrap();
        let sent = transport.submitted.lock().unwrap();
        assert_eq!(sent.len(), 4);
        assert!(sent.iter().all(|wire| wire["ou"] == "1000"));
        assert_eq!(*transport.receipts.lock().unwrap(), 2);
    }
}
