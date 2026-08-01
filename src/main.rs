fn main() {
    match octra_sqlite::cli::run_with_exit_code() {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(error) => {
            let message = format!("{error:#}");
            if wants_json_error() {
                eprintln!("{}", error_envelope(&error, &message));
            } else {
                eprintln!("error: {message}");
                if let Some(hint) = octra_sqlite::cli::error_hint(&error) {
                    eprintln!("hint: {hint}");
                }
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

fn error_envelope(error: &anyhow::Error, message: &str) -> serde_json::Value {
    let mut error_object = serde_json::json!({
        "code": octra_sqlite::cli::error_code(error),
        "message": message,
    });
    if let Some(details) = octra_sqlite::cli::error_details(error)
        && let Some(object) = error_object.as_object_mut()
    {
        object.insert("details".to_string(), details);
    }
    if let Some(hint) = octra_sqlite::cli::error_hint(error)
        && let Some(object) = error_object.as_object_mut()
    {
        object.insert(
            "hint".to_string(),
            serde_json::Value::String(hint.to_string()),
        );
    }
    serde_json::json!({
        "ok": false,
        "type": "error",
        "schema": "octra-sqlite.cli.v1",
        "exit_code": 1,
        "error": error_object,
    })
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
