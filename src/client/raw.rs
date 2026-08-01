//! Low-level Octra session and RPC plumbing.
//!
//! This module supports the CLI, audits, and adapters that need direct Circle
//! calls. Application query and write paths should prefer [`crate::Database`].

#[cfg(all(feature = "cli", feature = "http"))]
use super::write::prepare_write_with_owner_auth;
#[cfg(feature = "http")]
use super::{
    error::Result,
    results::AuthInfo,
    rpc::{
        auth_info_with, circle_info_with, next_nonce_with, program_info_with, query_typed_with,
        view_with, wait_for_receipt_with, wait_for_transaction_with,
    },
    safety::Operation,
    transport::{HttpTransport, RpcTraceMode, Transport},
    write::{DEFAULT_WRITE_OU, prepare_write_with_ou, sign_write, submit_signed_write_with},
};

pub use super::session::{
    ClientOptions, Session, build_control_session, build_session, resolve_database_target,
    resolve_wallet_path,
};
pub use super::wallet::{discover_wallet_path, wallet_caller};

#[cfg(feature = "cli")]
pub(crate) use super::wallet::{
    WalletMaterial, wallet_file_material, wallet_material_from_private_key,
};

#[cfg(feature = "http")]
use crate::protocol::tx::Tx;
#[cfg(feature = "http")]
use serde_json::Value;
#[cfg(feature = "http")]
use serde_json::json;

#[cfg(feature = "http")]
/// Call a Circle view method through the default HTTP transport.
pub fn view(session: &Session, method: &str, params: Vec<Value>) -> Result<Value> {
    let transport = HttpTransport::default();
    view_with(&transport, session, method, params)
}

#[cfg(feature = "http")]
/// Run read-only SQL and return the raw typed-result JSON envelope.
pub fn query_typed(session: &Session, sql: &str) -> Result<Value> {
    let transport = HttpTransport::default();
    query_typed_with(&transport, session, sql)
}

#[cfg(feature = "http")]
/// Run read-only SQL while writing a JSONL RPC trace.
pub fn query_typed_traced(
    session: &Session,
    sql: &str,
    trace_path: &std::path::Path,
    trace_mode: RpcTraceMode,
) -> Result<Value> {
    let transport = HttpTransport::with_trace_jsonl_mode(trace_path, trace_mode)?;
    query_typed_with(&transport, session, sql)
}

#[cfg(feature = "http")]
/// Read owner-write authorization metadata.
pub fn auth_info(session: &Session) -> Result<AuthInfo> {
    let transport = HttpTransport::default();
    auth_info_with(&transport, session)
}

#[cfg(feature = "http")]
/// Read raw deployed program metadata.
pub fn program_info(session: &Session) -> Result<Value> {
    let transport = HttpTransport::default();
    program_info_with(&transport, session)
}

#[cfg(feature = "http")]
/// Read raw Octra Circle metadata.
pub fn circle_info(session: &Session) -> Result<Value> {
    let transport = HttpTransport::default();
    circle_info_with(&transport, session)
}

#[cfg(feature = "http")]
/// Prepare, sign, and submit owner-write SQL through the default transport.
pub fn exec_sql(session: &Session, sql: &str, no_wait: bool) -> Result<Value> {
    exec_sql_with_ou(session, sql, no_wait, DEFAULT_WRITE_OU)
}

#[cfg(feature = "http")]
/// Prepare, sign, and submit owner-write SQL with an explicit OU budget.
pub fn exec_sql_with_ou(session: &Session, sql: &str, no_wait: bool, ou: &str) -> Result<Value> {
    let transport = HttpTransport::default();
    let operation = if no_wait {
        Operation::ExecuteNoWait
    } else {
        Operation::Execute
    };
    let prepared = prepare_write_with_ou(&transport, session, sql, operation, ou)?;
    let signed = sign_write(session, &prepared)?;
    submit_signed_write_with(&transport, session, signed, no_wait)
}

#[cfg(all(feature = "cli", feature = "http"))]
pub(crate) fn exec_sql_with_owner_auth_ou(
    session: &Session,
    sql: &str,
    db_id: &str,
    owner_pubkey: &str,
    ou: &str,
) -> Result<Value> {
    let transport = HttpTransport::default();
    let prepared = prepare_write_with_owner_auth(
        &transport,
        session,
        sql,
        Operation::Execute,
        db_id,
        owner_pubkey,
        ou,
    )?;
    let signed = sign_write(session, &prepared)?;
    submit_signed_write_with(&transport, session, signed, false)
}

#[cfg(feature = "http")]
/// Read the next transaction nonce for the session wallet.
pub fn next_nonce(session: &Session) -> Result<i64> {
    let transport = HttpTransport::default();
    next_nonce_with(&transport, session)
}

#[cfg(feature = "http")]
/// Sign and submit a generic Octra transaction.
pub fn submit_tx(session: &Session, tx: Tx, no_wait: bool) -> Result<Value> {
    let transport = HttpTransport::default();
    super::write::sign_and_submit_tx_with(&transport, session, tx, no_wait)
}

#[cfg(feature = "http")]
/// Wait for a transaction to reach a terminal receipt state.
pub fn wait_for_transaction(session: &Session, tx_hash: &str) -> Result<Value> {
    let transport = HttpTransport::default();
    wait_for_transaction_with(&transport, session, tx_hash)
}

#[cfg(feature = "http")]
/// Wait for a Circle contract receipt.
pub fn wait_for_receipt(session: &Session, tx_hash: &str) -> Result<Value> {
    let transport = HttpTransport::default();
    wait_for_receipt_with(&transport, session, tx_hash)
}

#[cfg(feature = "http")]
/// Read a transaction by hash.
pub fn transaction(session: &Session, tx_hash: &str) -> Result<Value> {
    let transport = HttpTransport::default();
    transport.call(session.rpc(), "octra_transaction", json!([tx_hash]))
}

#[cfg(feature = "http")]
/// Read transactions involving an address with RPC pagination.
pub fn transactions_by_address(
    session: &Session,
    address: &str,
    limit: u64,
    offset: u64,
) -> Result<Value> {
    let transport = HttpTransport::default();
    transport.call(
        session.rpc(),
        "octra_transactionsByAddress",
        json!([address, limit, offset]),
    )
}
