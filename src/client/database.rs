#[cfg(feature = "http")]
use super::transport::HttpTransport;
use super::{
    error::{Error, ErrorKind, Result},
    results::{AuthInfo, ExecuteResult, ProgramInfo, QueryResult, SubmittedTransaction},
    rpc::{auth_info_with, program_info_with, query_typed_with, wait_for_receipt_with},
    safety::Operation,
    session::{build_session, ClientOptions, Session},
    transport::Transport,
    write::{
        ensure_submit_mode, prepare_write_with, sign_write, submit_signed_write_with,
        PreparedWrite, SignedWrite,
    },
};
#[cfg(feature = "http")]
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "http")]
/// Configured client used to open Circle-backed SQLite databases.
#[derive(Clone)]
pub struct Client<T = HttpTransport> {
    options: ClientOptions,
    transport: Arc<T>,
}

#[cfg(not(feature = "http"))]
/// Configured client used to open Circle-backed SQLite databases.
#[derive(Clone)]
pub struct Client<T> {
    options: ClientOptions,
    transport: Arc<T>,
}

#[cfg(feature = "http")]
impl Default for Client<HttpTransport> {
    fn default() -> Self {
        Self {
            options: ClientOptions::default(),
            transport: Arc::new(HttpTransport::default()),
        }
    }
}

#[cfg(feature = "http")]
impl Client<HttpTransport> {
    /// Load the default local config and construct a client with the HTTP transport.
    ///
    /// This is fallible because it reads the configured octra-sqlite config
    /// file. Use [`Client::with_options`] when construction should be purely
    /// in-memory.
    pub fn from_default_config() -> Result<Self> {
        let config = super::config::load_config()?;
        let options = ClientOptions {
            target: config.default_database.clone(),
            wallet: config.wallet.as_ref().map(PathBuf::from),
            ..ClientOptions::default()
        };
        Ok(Self::with_options(options))
    }

    /// Construct a client from explicit options using the default HTTP transport.
    pub fn with_options(options: ClientOptions) -> Self {
        Self {
            options,
            transport: Arc::new(HttpTransport::default()),
        }
    }
}

impl<T: Transport> Client<T> {
    /// Construct a client from explicit options and a custom transport.
    pub fn with_transport(options: ClientOptions, transport: T) -> Self {
        Self {
            options,
            transport: Arc::new(transport),
        }
    }

    /// Open a database by saved name, Circle ID, or `oct://` URI.
    pub fn database(&self, target: impl Into<String>) -> Result<Database<T>> {
        let mut options = self.options.clone();
        options.target = Some(target.into());
        Database::open_with_shared_transport(options, Arc::clone(&self.transport))
    }
}

#[cfg(feature = "http")]
/// Opened Circle-backed SQLite database.
#[derive(Clone)]
pub struct Database<T = HttpTransport> {
    session: Session,
    transport: Arc<T>,
}

#[cfg(not(feature = "http"))]
/// Opened Circle-backed SQLite database.
#[derive(Clone)]
pub struct Database<T> {
    session: Session,
    transport: Arc<T>,
}

#[cfg(feature = "http")]
impl Database<HttpTransport> {
    /// Open a database directly with the default HTTP transport.
    pub fn open(options: ClientOptions) -> Result<Self> {
        Self::open_with_transport(options, HttpTransport::default())
    }
}

impl<T: Transport> Database<T> {
    /// Open a database directly with a custom transport.
    pub fn open_with_transport(options: ClientOptions, transport: T) -> Result<Self> {
        Self::open_with_shared_transport(options, Arc::new(transport))
    }

    fn open_with_shared_transport(options: ClientOptions, transport: Arc<T>) -> Result<Self> {
        Ok(Self {
            session: build_session(&options)?,
            transport,
        })
    }

    /// Run read-only SQL and return typed rows.
    pub fn query(&self, sql: &str) -> Result<QueryResult> {
        QueryResult::from_value(query_typed_with(
            self.transport.as_ref(),
            &self.session,
            sql,
        )?)
    }

    /// Submit a write and wait for its receipt.
    pub fn execute(&self, sql: &str) -> Result<ExecuteResult> {
        let prepared = self.prepare_write(sql)?;
        let signed = self.sign_write(&prepared)?;
        self.submit_signed_write_and_wait(signed)
    }

    /// Submit a write without waiting for confirmation.
    pub fn execute_no_wait(&self, sql: &str) -> Result<SubmittedTransaction> {
        let prepared = self.prepare_write_no_wait(sql)?;
        let signed = self.sign_write(&prepared)?;
        self.submit_signed_write(signed)
    }

    /// Prepare a write for later signing and no-wait submission.
    pub fn prepare_write_no_wait(&self, sql: &str) -> Result<PreparedWrite> {
        self.prepare_write_for(sql, Operation::ExecuteNoWait)
    }

    /// Prepare a write for later signing and confirmed execution.
    pub fn prepare_write(&self, sql: &str) -> Result<PreparedWrite> {
        self.prepare_write_for(sql, Operation::Execute)
    }

    fn prepare_write_for(&self, sql: &str, operation: Operation) -> Result<PreparedWrite> {
        prepare_write_with(self.transport.as_ref(), &self.session, sql, operation)
    }

    /// Sign a prepared write with the database session wallet.
    pub fn sign_write(&self, prepared: &PreparedWrite) -> Result<SignedWrite> {
        sign_write(&self.session, prepared)
    }

    /// Submit a signed write without waiting for confirmation.
    pub fn submit_signed_write(&self, signed: SignedWrite) -> Result<SubmittedTransaction> {
        ensure_submit_mode(&signed, Operation::ExecuteNoWait)?;
        SubmittedTransaction::from_value(submit_signed_write_with(
            self.transport.as_ref(),
            &self.session,
            signed,
            true,
        )?)
    }

    /// Submit a signed write and wait for its receipt.
    pub fn submit_signed_write_and_wait(&self, signed: SignedWrite) -> Result<ExecuteResult> {
        ensure_submit_mode(&signed, Operation::Execute)?;
        ExecuteResult::from_value(submit_signed_write_with(
            self.transport.as_ref(),
            &self.session,
            signed,
            false,
        )?)
    }

    /// Wait for a submitted transaction receipt.
    pub fn wait(&self, submitted: &SubmittedTransaction) -> Result<ExecuteResult> {
        let tx_hash = submitted.tx_hash.as_deref().ok_or_else(|| {
            Error::with_kind(
                ErrorKind::Config,
                "submitted transaction is missing tx_hash",
            )
        })?;
        let receipt = wait_for_receipt_with(self.transport.as_ref(), &self.session, tx_hash)?;
        let mut value = serde_json::Map::new();
        if let Some(circle) = &submitted.circle {
            value.insert(
                "circle".to_string(),
                serde_json::Value::String(circle.clone()),
            );
        }
        if let Some(wallet) = &submitted.wallet {
            value.insert(
                "wallet".to_string(),
                serde_json::Value::String(wallet.clone()),
            );
        }
        value.insert(
            "tx_hash".to_string(),
            serde_json::Value::String(tx_hash.to_string()),
        );
        value.insert("result".to_string(), submitted.result.clone());
        value.insert("receipt".to_string(), receipt);
        ExecuteResult::from_value(serde_json::Value::Object(value))
    }

    /// Read owner-write authorization metadata.
    pub fn auth_info(&self) -> Result<AuthInfo> {
        auth_info_with(self.transport.as_ref(), &self.session)
    }

    /// Read deployed Circle program metadata.
    pub fn program_info(&self) -> Result<ProgramInfo> {
        ProgramInfo::from_value(program_info_with(self.transport.as_ref(), &self.session)?)
    }
}

#[cfg(test)]
mod tests {
    use super::super::write::sign_and_submit_tx_with;
    use super::*;
    use crate::client::{Error, ErrorKind};
    use crate::protocol::tx::Tx;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockTransport {
        calls: Arc<Mutex<Vec<String>>>,
        receipt: Arc<Mutex<Value>>,
        public_info: bool,
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
                public_info: false,
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

        fn public_read_circle() -> Self {
            Self {
                public_info: true,
                ..Self::default()
            }
        }
    }

    impl Transport for MockTransport {
        fn call(&self, _rpc: &str, method: &str, params: Value) -> Result<Value> {
            self.calls.lock().unwrap().push(method.to_string());
            match method {
                "octra_circleInfo" => {
                    if self.public_info {
                        Ok(json!({
                            "privacy_class": "public",
                            "browser_mode": "gateway_allowed",
                            "resource_mode": "public_resources",
                        }))
                    } else {
                        Ok(json!({
                            "privacy_class": "sealed",
                            "browser_mode": "native_sealed",
                            "resource_mode": "sealed_read",
                        }))
                    }
                }
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
                "octra_circleView" => {
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
                _ => Err(Error::with_kind(
                    ErrorKind::Other,
                    format!("unexpected method {method}"),
                )),
            }
        }
    }

    struct ContractErrorTransport;

    impl Transport for ContractErrorTransport {
        fn call(&self, _rpc: &str, method: &str, _params: Value) -> Result<Value> {
            match method {
                "octra_circleViewAuth" => Ok(Value::String(
                    r#"{"ok":false,"error":"sqlite_prepare_failed","detail":"no such table: companion"}"#.to_string(),
                )),
                _ => Err(Error::with_kind(
                    ErrorKind::Other,
                    format!("unexpected method {method}"),
                )),
            }
        }
    }

    fn test_options() -> ClientOptions {
        ClientOptions {
            target: Some("oct://devnet/octABC?read_mode=sealed".to_string()),
            rpc: Some("mock://rpc".to_string()),
            caller: Some("octCaller".to_string()),
            private_key: Some(
                "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
            ),
            ..ClientOptions::default()
        }
    }

    fn test_options_for(target: &str) -> ClientOptions {
        ClientOptions {
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
    fn public_database_query_uses_unsigned_circle_view() {
        let transport = MockTransport::default();
        let calls = transport.calls.clone();
        let db = Database::open_with_transport(
            ClientOptions {
                target: Some("oct://devnet/octABC?read_mode=public".to_string()),
                rpc: Some("mock://rpc".to_string()),
                ..ClientOptions::default()
            },
            transport,
        )
        .unwrap();
        let result = db.query("select * from demo").unwrap();
        assert_eq!(result.row_count, 1);
        assert_eq!(calls.lock().unwrap().as_slice(), ["octra_circleView"]);
    }

    #[test]
    fn auto_database_query_detects_public_circle_without_wallet() {
        let transport = MockTransport::public_read_circle();
        let calls = transport.calls.clone();
        let db = Database::open_with_transport(
            ClientOptions {
                target: Some("oct://devnet/octABC?read_mode=auto".to_string()),
                rpc: Some("mock://rpc".to_string()),
                ..ClientOptions::default()
            },
            transport,
        )
        .unwrap();
        let result = db.query("select * from demo").unwrap();
        assert_eq!(result.row_count, 1);
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            ["octra_circleInfo", "octra_circleView"]
        );
    }

    #[test]
    fn database_query_surfaces_contract_sql_errors() {
        let db = Database::open_with_transport(test_options(), ContractErrorTransport).unwrap();
        let error = db.query("select * from companion;").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Rpc);
        assert!(error.to_string().contains("sqlite_prepare_failed"));
        assert!(error.to_string().contains("no such table: companion"));
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
        assert_eq!(error.kind(), ErrorKind::Receipt);
        assert!(error.to_string().contains("syntax error"));
        assert!(error.to_string().contains("tx_hash: abc123"));
    }

    #[test]
    fn signed_write_submit_mode_must_match_prepare_mode() {
        let transport = MockTransport::default();
        let db = Database::open_with_transport(test_options(), transport).unwrap();
        let prepared = db.prepare_write("create table demo(id integer);").unwrap();
        let signed = db.sign_write(&prepared).unwrap();
        let error = db.submit_signed_write(signed).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Config);
    }

    #[test]
    fn generic_tx_submit_allows_deploy_destination() {
        let transport = MockTransport::default();
        let calls = transport.calls.clone();
        let session = build_session(&test_options()).unwrap();
        let tx = Tx {
            from: session.caller().to_string(),
            to_: "octNewCircle".to_string(),
            amount: "0".to_string(),
            nonce: 42,
            ou: "1000".to_string(),
            timestamp: 1000.0,
            op_type: "deploy_circle".to_string(),
            encrypted_data: String::new(),
            message: "{}".to_string(),
            signature: String::new(),
            public_key: session.public_key_b64().unwrap().to_string(),
        };
        let result = sign_and_submit_tx_with(&transport, &session, tx, true).unwrap();
        assert_eq!(result["circle"], "octNewCircle");
        assert_eq!(result["tx_hash"], "abc123");
        assert_eq!(calls.lock().unwrap().as_slice(), ["octra_submit"]);
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
        assert_eq!(error.kind(), ErrorKind::Authorization);
    }
}
