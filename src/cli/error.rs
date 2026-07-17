use crate::client::{Error as ClientError, ErrorKind};
use anyhow::Error as AnyError;
use std::fmt;

#[derive(Debug)]
struct CodedError {
    code: &'static str,
    message: String,
}

impl fmt::Display for CodedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CodedError {}

pub(super) fn coded_error(code: &'static str, message: impl Into<String>) -> AnyError {
    AnyError::new(CodedError {
        code,
        message: message.into(),
    })
}

/// Return the stable CLI automation code carried by an error chain.
pub fn error_code(error: &AnyError) -> &'static str {
    for cause in error.chain() {
        if let Some(error) = cause.downcast_ref::<CodedError>() {
            return error.code;
        }
        if let Some(error) = cause.downcast_ref::<ClientError>() {
            return client_error_code(error);
        }
    }
    "command_failed"
}

fn client_error_code(error: &ClientError) -> &'static str {
    if let Some(code) = error.code() {
        return match code {
            "rpc_rate_limited" => "rpc_rate_limited",
            "rpc_non_json" => "rpc_non_json",
            "target_error" => "target_error",
            "storage_uninitialized" => "storage_uninitialized",
            "auth_uninitialized" => "auth_uninitialized",
            "result_limit_exceeded" => "result_limit_exceeded",
            "query_budget_exceeded" => "query_budget_exceeded",
            "exec_budget_exceeded" => "exec_budget_exceeded",
            "response_too_large" => "result_too_large",
            code if code.starts_with("sqlite_") => "sql_rejected",
            _ => default_client_error_code(error.kind()),
        };
    }
    default_client_error_code(error.kind())
}

fn default_client_error_code(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Authorization => "auth_failed",
        ErrorKind::Config => "config_error",
        ErrorKind::Decode | ErrorKind::Protocol => "decode_error",
        ErrorKind::Receipt => "circle_write_failed",
        ErrorKind::Rpc => "rpc_error",
        ErrorKind::Timeout => "timeout",
        ErrorKind::Transport => "rpc_unavailable",
        ErrorKind::Wallet => "wallet_error",
        ErrorKind::Io | ErrorKind::Other => "command_failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coded_cli_error_keeps_its_automation_code_through_context() {
        let error = coded_error("read_only", "SQL would write").context("running SQL");
        assert_eq!(error_code(&error), "read_only");
    }

    #[test]
    fn client_source_code_takes_precedence_over_error_kind() {
        let error = AnyError::new(ClientError::with_code(
            ErrorKind::Rpc,
            "query_budget_exceeded",
            "query exceeded deterministic SQLite work limit",
        ));
        assert_eq!(error_code(&error), "query_budget_exceeded");
    }

    #[test]
    fn client_error_kind_supplies_the_stable_fallback() {
        let error = AnyError::new(ClientError::with_kind(
            ErrorKind::Wallet,
            "private key unavailable",
        ));
        assert_eq!(error_code(&error), "wallet_error");
    }
}
