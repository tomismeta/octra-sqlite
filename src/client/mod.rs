mod config;
mod database;
mod error;
mod session;
mod transport;
mod wallet;

pub use config::{config_path, load_config, write_config, Config};
pub use database::{
    operation_safety, AuthInfo, Database, DatabaseOperation, ExecResult, OctraSqlite,
    OperationSafety, PreparedOwnerWrite, PreparedWrite, ProgramInfo, QueryResult, SignedWrite,
    SubmittedTx,
};
pub use error::{ClientError, ClientErrorKind, Result};
pub use session::SessionOptions;
pub use transport::{HttpTransport, Transport};

pub mod low_level {
    pub use super::database::{
        auth_info, exec_sql, next_nonce, program_info, query_typed, submit_tx, view,
        wait_for_transaction,
    };
    pub use super::session::{
        build_control_session, build_session, resolve_wallet_path, Session, SessionOptions,
    };
    pub use super::wallet::{discover_wallet_path, wallet_caller};
}
