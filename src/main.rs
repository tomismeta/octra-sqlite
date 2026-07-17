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
                            "code": octra_sqlite::cli::error_code(&error),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_errors_are_requested_by_summary_output_too() {
        assert!(is_json_error_arg("--json"));
        assert!(is_json_error_arg("--json-summary"));
        assert!(!is_json_error_arg("--trace-rpc-json"));
    }
}
