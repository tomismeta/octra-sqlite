use serde_json::Value;
use std::fs;
use std::process::{Command, Stdio};

fn octra_sqlite() -> Command {
    Command::new(env!("CARGO_BIN_EXE_octra-sqlite"))
}

#[test]
fn limits_json_is_machine_readable_without_wallet() {
    let output = octra_sqlite()
        .args(["limits", "--json"])
        .output()
        .expect("run octra-sqlite limits --json");
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["type"], "limits");
    assert_eq!(value["schema"], "octra-sqlite.cli.v1");
    assert_eq!(value["result"]["limit_error"], "result_limit_exceeded");
    assert_eq!(
        value["auth"]["auto_read_policy"],
        serde_json::json!({
            "field": "privacy_class",
            "public_value": "public",
        })
    );
    assert!(
        value["trace"]["modes"]
            .as_array()
            .unwrap()
            .contains(&Value::String("summary".to_string()))
    );
}

#[test]
fn commands_json_is_machine_readable_without_wallet() {
    let output = octra_sqlite()
        .args(["commands", "--json"])
        .output()
        .expect("run octra-sqlite commands --json");
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["type"], "commands");
    assert_eq!(value["schema"], "octra-sqlite.cli.v1");
    assert!(
        value["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command["command"] == "octra-sqlite DATABASE \"SQL\"")
    );
    assert!(
        value["json_envelopes"]
            .as_array()
            .unwrap()
            .contains(&Value::String("commands".to_string()))
    );
}

#[test]
fn json_errors_have_stable_shape_and_exit_code() {
    let output = octra_sqlite()
        .args(["check", "--json"])
        .stdin(Stdio::null())
        .output()
        .expect("run failing octra-sqlite check --json");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["type"], "error");
    assert_eq!(value["schema"], "octra-sqlite.cli.v1");
    assert_eq!(value["exit_code"], 1);
    assert!(value["error"]["code"].is_string());
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("check requires")
    );
}

#[test]
fn json_errors_keep_source_owned_limit_codes() {
    let oversized = "x".repeat(8_192);
    let output = octra_sqlite()
        .args(["check", "--json", "--sql", &oversized])
        .output()
        .expect("run oversized octra-sqlite check --json");
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "sql_too_large");

    let output = octra_sqlite()
        .args(["check", "--json", "--sql", "savepoint before_write;"])
        .output()
        .expect("run unsupported transaction check --json");
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "transactions_not_supported");
}

#[test]
fn wallet_target_and_database_errors_keep_their_stable_codes() {
    let home =
        std::env::temp_dir().join(format!("octra-sqlite-json-errors-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();

    let output = octra_sqlite()
        .args(["wallet", "import", "--json"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .output()
        .expect("run wallet import without a source");
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "wallet_error");

    let output = octra_sqlite()
        .args(["database", "info", "not-a-database-target", "--json"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .output()
        .expect("run database info with an invalid target");
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "target_error");

    let output = octra_sqlite()
        .args(["database", "set", "demo", "oct://devnet/octABC"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .output()
        .expect("save a database name");
    assert!(output.status.success());

    let output = octra_sqlite()
        .args(["new", "demo", "--json"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .output()
        .expect("run new with an existing database name");
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "target_error");

    let missing_wallet = home.join("missing-wallet.json");
    let output = octra_sqlite()
        .args(["new", "fresh", "--json", "--wallet"])
        .arg(&missing_wallet)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .output()
        .expect("run new with a missing wallet path");
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "wallet_error");

    fs::remove_dir_all(home).unwrap();
}
