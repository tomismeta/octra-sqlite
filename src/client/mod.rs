//! Advanced client configuration, transport, and signing building blocks.
//!
//! Most applications should begin with [`crate::Client`] and
//! [`crate::Database`]. This module exposes custom transports, local config,
//! operation safety metadata, and the prepare/sign/submit write lifecycle.
//! Direct session and RPC helpers live in [`raw`].

mod config;
mod database;
mod error;
pub mod raw;
mod results;
mod rpc;
mod safety;
mod session;
mod transport;
mod wallet;
mod write;

pub use config::{Config, DatabaseMetadata, NetworkConfig, config_path, load_config, write_config};
pub use database::{Client, Database};
pub use error::{Error, ErrorKind, Result};
pub use results::{
    AuthInfo, ExecuteResult, ProgramInfo, QueryResult, StorageInfo, SubmittedTransaction,
};
#[cfg(feature = "cli")]
pub(crate) use rpc::circle_info_allows_unsigned_read;
pub use safety::{Operation, OperationSafety};
pub use session::ClientOptions;
pub use transport::Transport;
#[cfg(feature = "http")]
pub use transport::{HttpTransport, RpcTraceMode};
pub use write::{PreparedOwnerWrite, PreparedWrite, SignedWrite};
