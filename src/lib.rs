//! Real SQLite inside an Octra Circle.
//!
//! `octra-sqlite` provides a small Rust client for querying and writing to a
//! SQLite database whose engine runs inside an Octra `wasm_v1` Circle. The
//! crate keeps the first story deliberately small: create a [`Client`], open a
//! [`Database`], then run SQL.
//!
//! ```no_run
//! use octra_sqlite::{Client, Result};
//!
//! fn main() -> Result<()> {
//!     let client = Client::from_default_config()?;
//!     let db = client.database("art")?;
//!     let rows = db.query("select * from artist order by name;")?;
//!     println!("{} rows", rows.row_count);
//!     Ok(())
//! }
//! ```
//!
//! Sealed databases use signed Octra view auth for reads. Public-read
//! databases use unsigned Octra Circle views for SQL reads while keeping writes
//! owner-signed through OSW1 owner write intent. Pass a saved database name or a
//! full `oct://NETWORK/<circle>?read_mode=public` URI to [`Client::database`].
//!
//! Feature flags:
//!
//! - `cli`: build the `octra-sqlite` command line interface.
//! - `http`: include the default blocking HTTP RPC transport.
//! - `wasm-behavior`: enable host-harness tests for the bundled Circle WASM.
//!
//! The CLI JSON envelopes and OSR1/OSW1 wire formats are treated as public
//! surfaces. The Rust API is still `0.x`; breaking Rust API cleanup happens in
//! minor versions.

pub mod client;
pub mod protocol;

pub use client::{
    AuthInfo, Client, ClientOptions, Database, Error, ErrorKind, ExecuteResult, ProgramInfo,
    QueryResult, Result, SubmittedTransaction,
};
pub use protocol::target::ReadMode;
pub use serde_json::Value;

#[cfg(feature = "cli")]
#[path = "cli/mod.rs"]
pub mod cli;
