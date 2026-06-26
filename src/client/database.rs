use super::{
    error::Result,
    results::{AuthInfo, ExecResult, ProgramInfo, QueryResult, SubmittedTx},
    rpc::{
        auth_info_with, next_nonce_with, program_info_with, query_typed_with, sign_canonical_tx,
        view_with, wait_for_transaction_with,
    },
    safety::{operation_safety, DatabaseOperation},
    session::{build_session, Session, SessionOptions},
    transport::{HttpTransport, Transport},
    write::{
        ensure_submit_mode, prepare_write_with, sign_write, submit_signed_write_with,
        PreparedWrite, SignedWrite,
    },
};
use crate::protocol::tx::Tx;
use serde_json::Value;
use std::sync::Arc;

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
    sign_canonical_tx(session, &mut tx)?;
    let signed = SignedWrite::new(
        tx,
        operation_safety(if no_wait {
            DatabaseOperation::ExecuteNoWait
        } else {
            DatabaseOperation::Execute
        }),
    );
    submit_signed_write_with(&transport, session, signed, no_wait)
}

pub fn wait_for_transaction(session: &Session, tx_hash: &str) -> Result<Value> {
    let transport = HttpTransport::default();
    wait_for_transaction_with(&transport, session, tx_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ClientError, ClientErrorKind};
    use serde_json::{json, Value};
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
}
