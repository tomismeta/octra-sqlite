// This test is an ontology tripwire. The imports are the assertion.
#![allow(unused_imports)]

use octra_sqlite::{
    AuthInfo, Client, ClientOptions, Database, Error, ErrorKind, ExecuteResult, ProgramInfo,
    QueryResult, ReadMode, Result, SubmittedTransaction, Value,
};

use octra_sqlite::client::{
    config_path, load_config, write_config, Config, DatabaseMetadata, NetworkConfig, Operation,
    OperationSafety, PreparedOwnerWrite, PreparedWrite, SignedWrite, Transport,
};

#[cfg(feature = "http")]
use octra_sqlite::client::{HttpTransport, RpcTraceMode};

use octra_sqlite::client::raw::{
    build_control_session, build_session, discover_wallet_path, resolve_database_target,
    resolve_wallet_path, wallet_caller, ClientOptions as RawClientOptions, Session,
};

#[cfg(feature = "http")]
use octra_sqlite::client::raw::{
    auth_info, circle_info, exec_sql, next_nonce, program_info, query_typed, query_typed_traced,
    submit_tx, view, wait_for_transaction,
};

use octra_sqlite::protocol::{error, osr1, osw1, target, tx};

#[test]
fn public_surface_imports_compile() {
    assert_eq!(ReadMode::Public.as_str(), "public");
    assert!(Operation::Execute.safety().submits_transaction);
}
