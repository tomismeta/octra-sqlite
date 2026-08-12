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
