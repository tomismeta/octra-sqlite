fn main() {
    if let Err(error) = octra_sqlite::run_cli() {
        let message = format!("{error:#}");
        if wants_json_error() {
            eprintln!(
                "{}",
                serde_json::json!({
                    "ok": false,
                    "type": "error",
                    "schema": "octra-sqlite.cli.v1",
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

fn wants_json_error() -> bool {
    std::env::args().any(|arg| arg == "--json")
}

fn classify_error(message: &str) -> &'static str {
    if message.contains("Octra SQLite accepts at most")
        || message.contains("SQL payload")
        || message.contains("SQL statement")
    {
        "sql_too_large"
    } else if message.contains("transactions_not_supported") {
        "transactions_not_supported"
    } else if message.contains("read_only") {
        "read_only"
    } else if message.contains("sqlite_") || message.contains("database error") {
        "database_error"
    } else if message.contains("wallet") {
        "wallet_error"
    } else if message.contains("unknown database")
        || message.contains("resolving database")
        || message.contains("database URI")
    {
        "target_error"
    } else if message.contains("RPC") || message.contains("rpc") {
        "rpc_error"
    } else {
        "command_failed"
    }
}
