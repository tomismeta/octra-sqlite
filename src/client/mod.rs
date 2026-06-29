mod config;
mod database;
mod error;
mod results;
mod rpc;
mod safety;
mod session;
mod transport;
mod wallet;
mod write;

pub use config::{config_path, load_config, write_config, Config};
pub use database::{Database, OctraSqlite};
pub use error::{ClientError, ClientErrorKind, Result};
pub use results::{AuthInfo, ExecResult, ProgramInfo, QueryResult, SubmittedTx};
pub use safety::{operation_safety, DatabaseOperation, OperationSafety};
pub use session::SessionOptions;
#[cfg(feature = "http")]
pub use transport::HttpTransport;
pub use transport::Transport;
pub use write::{PreparedOwnerWrite, PreparedWrite, SignedWrite};

pub mod low_level {
    #[cfg(feature = "http")]
    pub use super::database::{
        auth_info, exec_sql, next_nonce, program_info, query_typed, query_typed_traced, submit_tx,
        view, wait_for_transaction,
    };
    pub use super::session::{
        build_control_session, build_session, resolve_wallet_path, Session, SessionOptions,
    };
    pub use super::wallet::{discover_wallet_path, wallet_caller};
}
