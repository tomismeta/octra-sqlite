use std::fs;
use std::path::Path;

#[test]
fn protocol_layer_stays_transport_and_cli_free() {
    let forbidden = [
        "anyhow",
        "ureq",
        "clap",
        "dirs",
        "std::env",
        "std::fs",
        "PathBuf",
        "println!",
        "eprintln!",
    ];
    for path in source_files("src/protocol") {
        let text = fs::read_to_string(&path).unwrap();
        for needle in forbidden {
            assert!(
                !text.contains(needle),
                "{} must not contain {needle}",
                path.display()
            );
        }
    }
}

#[test]
fn client_layer_does_not_depend_on_cli_rendering() {
    let forbidden = [
        "clap",
        "crate::cli::",
        "crate::cli;",
        "cli::",
        "OutputMode",
        "print_result",
    ];
    for path in source_files("src/client") {
        let text = fs::read_to_string(&path).unwrap();
        for needle in forbidden {
            assert!(
                !text.contains(needle),
                "{} must not contain {needle}",
                path.display()
            );
        }
    }
}

#[test]
fn cli_orchestrator_delegates_owned_workflows() {
    let orchestrator = fs::read_to_string("src/cli/mod.rs").unwrap();
    for module in [
        "catalog",
        "databases",
        "deployment",
        "inspection",
        "onboarding",
        "sql",
        "tests",
    ] {
        assert!(
            orchestrator.contains(&format!("mod {module};")),
            "CLI orchestrator must retain the {module} ownership boundary"
        );
    }
    for workflow in [
        "fn cmd_setup(",
        "fn cmd_new(",
        "fn cmd_database(",
        "fn cmd_open(",
        "fn cmd_limits(",
        "fn cmd_status(",
        "fn cmd_deploy(",
    ] {
        assert!(
            !orchestrator.contains(workflow),
            "{workflow} belongs in its owned CLI module"
        );
    }
}

#[test]
fn ordinary_cli_sql_uses_the_database_data_plane() {
    let sql = fs::read_to_string("src/cli/sql.rs").unwrap();
    assert!(sql.contains("Database::from_session(session.clone()).query_value(sql)"));
    assert!(sql.contains("Database::from_session(session.clone()).execute_value(sql, false)"));
}

fn source_files(dir: impl AsRef<Path>) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    collect_source_files(dir.as_ref(), &mut out);
    out
}

fn collect_source_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_source_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
