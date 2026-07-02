fn main() {
    match octra_sqlite::cli::run_with_exit_code() {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(error) => {
            let message = format!("{error:#}");
            if wants_json_error() {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "ok": false,
                        "type": "error",
                        "schema": "octra-sqlite.cli.v1",
                        "exit_code": 1,
                        "error": {
                            "code": classify_error(&message),
                            "message": message,
                        }
                    })
                );
            } else {
                eprintln!("error: {message}");
            }
            std::process::exit(1);
        }
    }
}

fn wants_json_error() -> bool {
    std::env::args().any(|arg| is_json_error_arg(&arg))
}

fn is_json_error_arg(arg: &str) -> bool {
    arg == "--json" || arg == "--json-summary"
}

fn classify_error(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("http 429")
        || lower.contains("too many requests")
        || lower.contains("rate limit")
        || lower.contains("rate-limited")
    {
        "rpc_rate_limited"
    } else if lower.contains("non-json") || lower.contains("html") {
        "rpc_non_json"
    } else if message.contains("bootstrap first write was submitted")
        && message.contains("post-write auth_info")
    {
        "bootstrap_unverified"
    } else if lower.contains("already_bootstrapped")
        || lower.contains("already bootstrapped")
        || lower.contains("auth_info is already readable")
    {
        "bootstrap_already_done"
    } else if lower.contains("missing storage cache") || lower.contains("storage_uninitialized") {
        "storage_uninitialized"
    } else if lower.contains("auth_uninitialized") || lower.contains("auth_info") {
        "auth_uninitialized"
    } else if message.contains("Octra SQLite accepts at most")
        || message.contains("SQL payload")
        || message.contains("SQL statement")
    {
        "sql_too_large"
    } else if message.contains("transactions_not_supported") {
        "transactions_not_supported"
    } else if message.contains("read_only") {
        "read_only"
    } else if message.contains("result_limit_exceeded") {
        "result_limit_exceeded"
    } else if message.contains("response_too_large") {
        "result_too_large"
    } else if message.contains("Authorization")
        || message.contains("authorization")
        || message.contains("owner")
        || message.contains("not owner")
    {
        "auth_failed"
    } else if message.contains("SQL execution failed")
        || message.contains("circle_call_failed")
        || message.contains("receipt")
    {
        "circle_write_failed"
    } else if message.contains("sqlite_") || message.contains("database error") {
        "sql_rejected"
    } else if message.contains("wallet") {
        "wallet_error"
    } else if message.contains("unknown database")
        || message.contains("resolving database")
        || message.contains("database URI")
    {
        "target_error"
    } else if message.contains("timed out") || message.contains("timeout") {
        "timeout"
    } else if message.contains("decoding") || message.contains("decode") {
        "decode_error"
    } else if message.contains("calling ")
        || message.contains("HTTP")
        || message.contains("transport")
    {
        "rpc_unavailable"
    } else if message.contains("RPC") || message.contains("rpc") {
        "rpc_error"
    } else if message.contains("config") || message.contains("Config") {
        "config_error"
    } else {
        "command_failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_errors_are_requested_by_summary_output_too() {
        assert!(is_json_error_arg("--json"));
        assert!(is_json_error_arg("--json-summary"));
        assert!(!is_json_error_arg("--trace-rpc-json"));
    }

    #[test]
    fn error_classification_uses_stable_automation_codes() {
        assert_eq!(
            classify_error("database error (result_limit_exceeded): query returned too many rows"),
            "result_limit_exceeded"
        );
        assert_eq!(
            classify_error("database error (response_too_large): typed query result exceeded contract response buffer"),
            "result_too_large"
        );
        assert_eq!(
            classify_error("database error (sqlite_prepare_failed): no such table: demo"),
            "sql_rejected"
        );
        assert_eq!(
            classify_error("read_only: SQL would write; remove --read-only"),
            "read_only"
        );
        assert_eq!(
            classify_error("calling octra_circleViewAuth: connection refused"),
            "rpc_unavailable"
        );
        assert_eq!(
            classify_error("octra_circleViewAuth failed with HTTP 429 Too Many Requests"),
            "rpc_rate_limited"
        );
        assert_eq!(
            classify_error("decoding octra_circleViewAuth non-JSON response from HTTP 503"),
            "rpc_non_json"
        );
        assert_eq!(
            classify_error("octra_circleViewAuth failed: missing storage cache: octABC:0000"),
            "storage_uninitialized"
        );
        assert_eq!(
            classify_error(
                "bootstrap first write was submitted but post-write auth_info still failed"
            ),
            "bootstrap_unverified"
        );
    }
}
