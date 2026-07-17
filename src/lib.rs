#![warn(missing_docs)]

//! Real SQLite inside an Octra Circle.
//!
//! `octra-sqlite` provides a small Rust client for querying and writing to a
//! SQLite database whose engine runs inside an Octra `wasm_v1` Circle. The
//! crate keeps the first story deliberately small: create a [`Client`], open a
//! [`Database`], then run SQL.
//!
//! # Start here
//!
//! Public-read databases need no local wallet or config:
//!
//! ```no_run
//! use octra_sqlite::{Client, Result};
//!
//! fn main() -> Result<()> {
//!     let client = Client::default();
//!     let db = client.database(
//!         "oct://devnet/octQfYK2fE9RvR9kfj8FJfMBQw1e4EzfHB8Q5Z9J2DCnRBQ",
//!     )?;
//!     let rows = db.query("select id, name from artist order by id;")?;
//!     println!("{} rows", rows.row_count);
//!     Ok(())
//! }
//! ```
//!
//! # Configured databases and writes
//!
//! [`Client::from_default_config`] loads the same saved database and wallet
//! configuration used by the CLI. Writes are owner-signed and confirmed by
//! default:
//!
//! ```no_run
//! use octra_sqlite::{Client, Result};
//!
//! fn main() -> Result<()> {
//!     let client = Client::from_default_config()?;
//!     let db = client.database("art")?;
//!     let result = db.execute("insert into artist(name) values ('Hokusai');")?;
//!     println!("confirmed: {}", result.submitted.tx_hash.is_some());
//!     Ok(())
//! }
//! ```
//!
//! Use [`Database::execute_no_wait`] when submission and confirmation must be
//! separate, then complete the lifecycle with [`Database::wait`].
//!
//! # Read and write model
//!
//! Sealed databases use signed Octra view auth for reads. Public-read
//! databases use unsigned Octra Circle views for SQL reads while keeping writes
//! owner-signed through OSW1 owner write intent. Pass a saved database name or a
//! full `oct://NETWORK/<circle>` URI to [`Client::database`]. The client
//! detects the Circle's Octra read surface unless `read_mode` is explicitly set.
//! Sealed authenticates reads; it does not encrypt data or make reads owner-only.
//! Write SQL and values are visible in Octra transaction history.
//!
//! # API map
//!
//! - [`Client`] owns configuration and transport; [`Database`] owns one opened
//!   SQL data plane.
//! - [`QueryResult`], [`ExecuteResult`], and [`SubmittedTransaction`] model the
//!   query, confirmed-write, and submitted-write lifecycles.
//! - [`AuthInfo`], [`ProgramInfo`], and [`ReadMode`] expose database and Circle
//!   state without leaking raw RPC details into the first story.
//! - [`client`] contains advanced transport, config, and signing types.
//! - [`client::raw`] contains lower-level Octra RPC plumbing for adapters and
//!   operational tooling.
//! - [`protocol`] contains the transport-independent OSR1, OSW1, target, and
//!   transaction wire formats.
//!
//! # Errors
//!
//! [`Error::kind`] is the stable broad category for application handling.
//! [`Error::code`] preserves a more precise machine-readable code supplied by
//! the RPC, Circle, or receipt, or assigned at a local protocol boundary.
//! Human error text is not an automation contract.
//!
//! # Build configuration
//!
//! - `cli`: build the `octra-sqlite` command line interface.
//! - `http`: include the default blocking HTTP RPC transport.
//! - `wasm-behavior`: enable host-harness tests for the bundled Circle WASM.
//!
//! docs.rs builds the library with `http` and without the CLI. The default
//! crate configuration includes both `cli` and `http`.
//!
//! # Stability
//!
//! The CLI JSON envelopes and OSR1/OSW1 wire formats are treated as public
//! surfaces. The Rust API is still `0.x`; breaking Rust API cleanup happens in
//! minor versions.

pub mod client;
mod private_file;
pub mod protocol;

pub use client::{
    AuthInfo, Client, ClientOptions, Database, Error, ErrorKind, ExecuteResult, ProgramInfo,
    QueryResult, Result, SubmittedTransaction,
};
pub use protocol::target::ReadMode;
pub use serde_json::Value;

#[cfg(feature = "cli")]
#[path = "cli/mod.rs"]
/// Human and automation CLI entrypoints.
pub mod cli;
