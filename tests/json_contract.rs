use serde_json::Value;
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
    assert!(value["trace"]["modes"]
        .as_array()
        .unwrap()
        .contains(&Value::String("summary".to_string())));
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
    assert!(value["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["command"] == "octra-sqlite DATABASE \"SQL\""));
    assert!(value["json_envelopes"]
        .as_array()
        .unwrap()
        .contains(&Value::String("commands".to_string())));
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
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains("check requires"));
}
