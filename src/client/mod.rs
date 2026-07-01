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

pub use config::{config_path, load_config, write_config, Config, DatabaseMetadata};
pub use database::{Database, OctraSqlite};
pub use error::{ClientError, ClientErrorKind, Result};
pub use results::{AuthInfo, ExecResult, ProgramInfo, QueryResult, SubmittedTx};
pub use safety::{operation_safety, DatabaseOperation, OperationSafety};
pub use session::SessionOptions;
pub use transport::Transport;
#[cfg(feature = "http")]
pub use transport::{HttpTransport, RpcTraceMode};
pub use write::{PreparedOwnerWrite, PreparedWrite, SignedWrite};

pub mod low_level {
    #[cfg(feature = "http")]
    pub(crate) use super::database::exec_sql_with_owner_auth;
    #[cfg(feature = "http")]
    pub use super::database::{
        auth_info, circle_info, exec_sql, next_nonce, program_info, query_typed,
        query_typed_traced, submit_tx, view, wait_for_transaction,
    };
    pub use super::session::{
        build_control_session, build_session, resolve_wallet_path, Session, SessionOptions,
    };
    pub use super::wallet::{discover_wallet_path, wallet_caller};
    #[cfg(feature = "cli")]
    pub(crate) use super::wallet::{
        wallet_file_material, wallet_material_from_private_key, WalletMaterial,
    };
}
