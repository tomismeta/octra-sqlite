#[cfg(feature = "http")]
use super::write::prepare_write_with_owner_auth;
#[cfg(feature = "http")]
use super::{
    error::Result,
    results::AuthInfo,
    rpc::{
        auth_info_with, circle_info_with, next_nonce_with, program_info_with, query_typed_with,
        view_with, wait_for_transaction_with,
    },
    safety::Operation,
    transport::{HttpTransport, RpcTraceMode},
    write::{prepare_write_with, sign_write, submit_signed_write_with},
};

pub use super::session::{
    build_control_session, build_session, resolve_database_target, resolve_wallet_path,
    ClientOptions, Session,
};
pub use super::wallet::{discover_wallet_path, wallet_caller};

#[cfg(feature = "cli")]
pub(crate) use super::wallet::{
    wallet_file_material, wallet_material_from_private_key, WalletMaterial,
};

#[cfg(feature = "http")]
use crate::protocol::tx::Tx;
#[cfg(feature = "http")]
use serde_json::Value;

#[cfg(feature = "http")]
pub fn view(session: &Session, method: &str, params: Vec<Value>) -> Result<Value> {
    let transport = HttpTransport::default();
    view_with(&transport, session, method, params)
}

#[cfg(feature = "http")]
pub fn query_typed(session: &Session, sql: &str) -> Result<Value> {
    let transport = HttpTransport::default();
    query_typed_with(&transport, session, sql)
}

#[cfg(feature = "http")]
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
pub fn auth_info(session: &Session) -> Result<AuthInfo> {
    let transport = HttpTransport::default();
    auth_info_with(&transport, session)
}

#[cfg(feature = "http")]
pub fn program_info(session: &Session) -> Result<Value> {
    let transport = HttpTransport::default();
    program_info_with(&transport, session)
}

#[cfg(feature = "http")]
pub fn circle_info(session: &Session) -> Result<Value> {
    let transport = HttpTransport::default();
    circle_info_with(&transport, session)
}

#[cfg(feature = "http")]
pub fn exec_sql(session: &Session, sql: &str, no_wait: bool) -> Result<Value> {
    let transport = HttpTransport::default();
    let operation = if no_wait {
        Operation::ExecuteNoWait
    } else {
        Operation::Execute
    };
    let prepared = prepare_write_with(&transport, session, sql, operation)?;
    let signed = sign_write(session, &prepared)?;
    submit_signed_write_with(&transport, session, signed, no_wait)
}

#[cfg(feature = "http")]
pub(crate) fn exec_sql_with_owner_auth(
    session: &Session,
    sql: &str,
    db_id: &str,
    owner_pubkey: &str,
) -> Result<Value> {
    let transport = HttpTransport::default();
    let prepared = prepare_write_with_owner_auth(
        &transport,
        session,
        sql,
        Operation::Execute,
        db_id,
        owner_pubkey,
    )?;
    let signed = sign_write(session, &prepared)?;
    submit_signed_write_with(&transport, session, signed, false)
}

#[cfg(feature = "http")]
pub fn next_nonce(session: &Session) -> Result<i64> {
    let transport = HttpTransport::default();
    next_nonce_with(&transport, session)
}

#[cfg(feature = "http")]
pub fn submit_tx(session: &Session, tx: Tx, no_wait: bool) -> Result<Value> {
    let transport = HttpTransport::default();
    super::write::sign_and_submit_tx_with(&transport, session, tx, no_wait)
}

#[cfg(feature = "http")]
pub fn wait_for_transaction(session: &Session, tx_hash: &str) -> Result<Value> {
    let transport = HttpTransport::default();
    wait_for_transaction_with(&transport, session, tx_hash)
}
