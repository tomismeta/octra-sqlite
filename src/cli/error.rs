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

pub(super) fn auth_error(message: impl Into<String>) -> AnyError {
    coded_error("auth_failed", message)
}

pub(super) fn with_fallback_code(error: AnyError, code: &'static str) -> AnyError {
    match classified_error_code(&error) {
        Some(existing) if existing != "command_failed" => error,
        _ => coded_error(code, format!("{error:#}")),
    }
}

/// Return the stable CLI automation code for an error chain.
pub fn error_code(error: &AnyError) -> &'static str {
    classified_error_code(error).unwrap_or("command_failed")
}

fn classified_error_code(error: &AnyError) -> Option<&'static str> {
    for cause in error.chain() {
        if let Some(error) = cause.downcast_ref::<CodedError>() {
            return Some(error.code);
        }
        if let Some(error) = cause.downcast_ref::<ClientError>() {
            return Some(client_error_code(error));
        }
    }
    None
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
            code if code.starts_with("auth_") => "auth_failed",
            code if code.starts_with("sqlite_") && error.kind() != ErrorKind::Receipt => {
                "sql_rejected"
            }
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

    #[test]
    fn submitted_sqlite_failure_remains_a_circle_write_failure() {
        let error = AnyError::new(ClientError::with_code(
            ErrorKind::Receipt,
            "sqlite_exec_failed",
            "SQL execution failed",
        ));
        assert_eq!(error_code(&error), "circle_write_failed");
    }

    #[test]
    fn command_fallback_codes_only_untyped_errors() {
        let untyped =
            with_fallback_code(anyhow::anyhow!("wallet path is required"), "wallet_error");
        assert_eq!(error_code(&untyped), "wallet_error");
        assert_eq!(untyped.to_string(), "wallet path is required");

        let typed = AnyError::new(ClientError::with_kind(
            ErrorKind::Config,
            "config could not be read",
        ));
        let typed = with_fallback_code(typed, "wallet_error");
        assert_eq!(error_code(&typed), "config_error");
    }

    #[test]
    fn stable_error_vocabulary_has_typed_classifications() {
        for code in [
            "sql_too_large",
            "transactions_not_supported",
            "read_only",
            "bootstrap_unverified",
            "bootstrap_already_done",
        ] {
            assert_eq!(error_code(&coded_error(code, code)), code);
        }

        for (source, expected) in [
            ("result_limit_exceeded", "result_limit_exceeded"),
            ("query_budget_exceeded", "query_budget_exceeded"),
            ("exec_budget_exceeded", "exec_budget_exceeded"),
            ("response_too_large", "result_too_large"),
            ("sqlite_prepare_failed", "sql_rejected"),
            ("auth_denied", "auth_failed"),
            ("storage_uninitialized", "storage_uninitialized"),
            ("auth_uninitialized", "auth_uninitialized"),
            ("target_error", "target_error"),
            ("rpc_rate_limited", "rpc_rate_limited"),
            ("rpc_non_json", "rpc_non_json"),
        ] {
            let error = AnyError::new(ClientError::with_code(ErrorKind::Rpc, source, source));
            assert_eq!(error_code(&error), expected, "source code {source}");
        }

        for (kind, expected) in [
            (ErrorKind::Authorization, "auth_failed"),
            (ErrorKind::Receipt, "circle_write_failed"),
            (ErrorKind::Wallet, "wallet_error"),
            (ErrorKind::Timeout, "timeout"),
            (ErrorKind::Decode, "decode_error"),
            (ErrorKind::Transport, "rpc_unavailable"),
            (ErrorKind::Rpc, "rpc_error"),
            (ErrorKind::Config, "config_error"),
            (ErrorKind::Other, "command_failed"),
        ] {
            let error = AnyError::new(ClientError::with_kind(kind, expected));
            assert_eq!(error_code(&error), expected, "error kind {kind:?}");
        }
    }
}
