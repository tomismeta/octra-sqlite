use super::*;
use std::collections::BTreeSet;

#[test]
fn parses_oct_target() {
    let target = parse_target_uri("oct://devnet/octABC", &Config::default()).unwrap();
    assert_eq!(target.network, "devnet");
    assert_eq!(target.circle, "octABC");
}

#[test]
fn normalizes_bare_target_to_open() {
    let args = normalize_args(vec![
        "octra-sqlite".into(),
        "my-db".into(),
        "select 1;".into(),
    ]);
    assert_eq!(args[1], "open");
    assert_eq!(args[2], "my-db");
}

#[test]
fn knows_new_is_a_top_level_command() {
    let args = normalize_args(vec!["octra-sqlite".into(), "new".into(), "my-db".into()]);
    assert_eq!(args[1], "new");
}

#[test]
fn database_command_is_the_public_name_command() {
    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "database",
        "set",
        "organization",
        "oct://devnet/octABC",
    ])
    .unwrap();
    match cli.command {
        Commands::Database { command } => match command {
            DatabaseCommand::Set { name, database } => {
                assert_eq!(name, "organization");
                assert_eq!(database, "oct://devnet/octABC");
            }
            _ => panic!("expected database set command"),
        },
        _ => panic!("expected database command"),
    }
}

#[test]
fn database_info_is_discoverable() {
    let cli = Cli::try_parse_from(["octra-sqlite", "database", "info", "organization"]).unwrap();
    match cli.command {
        Commands::Database { command } => match command {
            DatabaseCommand::Info { database, json } => {
                assert_eq!(database.as_deref(), Some("organization"));
                assert!(!json);
            }
            _ => panic!("expected database info command"),
        },
        _ => panic!("expected database command"),
    }
}

#[test]
fn database_default_is_the_public_default_command() {
    let cli = Cli::try_parse_from(["octra-sqlite", "database", "default", "organization"]).unwrap();
    match cli.command {
        Commands::Database { command } => match command {
            DatabaseCommand::Default { name } => {
                assert_eq!(name, "organization");
            }
            _ => panic!("expected database default command"),
        },
        _ => panic!("expected database command"),
    }
}

#[test]
fn status_and_config_are_public_commands() {
    let status = Cli::try_parse_from(["octra-sqlite", "status", "--skip-network"]).unwrap();
    match status.command {
        Commands::Status(args) => {
            assert!(args.skip_network);
            assert!(!args.json);
        }
        _ => panic!("expected status command"),
    }

    let config = Cli::try_parse_from(["octra-sqlite", "config", "--json"]).unwrap();
    match config.command {
        Commands::Config(args) => assert!(args.json),
        _ => panic!("expected config command"),
    }
}

#[test]
fn restore_check_and_limits_are_public_commands() {
    let restore = Cli::try_parse_from([
        "octra-sqlite",
        "restore",
        "art",
        "--file",
        "dump.sql",
        "--ou",
        "200000",
        "--json",
    ])
    .unwrap();
    match restore.command {
        Commands::Restore(args) => {
            assert_eq!(args.target.target.as_deref(), Some("art"));
            assert_eq!(args.file.as_deref(), Some(Path::new("dump.sql")));
            assert_eq!(args.ou.as_deref(), Some("200000"));
            assert!(args.json);
        }
        _ => panic!("expected restore command"),
    }

    let check =
        Cli::try_parse_from(["octra-sqlite", "check", "art", "--sql-file", "-", "--json"]).unwrap();
    match check.command {
        Commands::Check(args) => {
            assert_eq!(args.target.target.as_deref(), Some("art"));
            assert_eq!(args.sql_file.as_deref(), Some(Path::new("-")));
            assert!(args.json);
        }
        _ => panic!("expected check command"),
    }

    let limits = Cli::try_parse_from(["octra-sqlite", "limits", "art", "--json"]).unwrap();
    match limits.command {
        Commands::Limits(args) => {
            assert_eq!(args.target.target.as_deref(), Some("art"));
            assert!(args.json);
        }
        _ => panic!("expected limits command"),
    }

    let commands = Cli::try_parse_from(["octra-sqlite", "commands", "--json"]).unwrap();
    match commands.command {
        Commands::CommandList(args) => assert!(args.json),
        _ => panic!("expected commands command"),
    }

    let receipt =
        Cli::try_parse_from(["octra-sqlite", "receipt", "abc123", "art", "--json"]).unwrap();
    match receipt.command {
        Commands::Receipt(args) => {
            assert_eq!(args.tx_hash, "abc123");
            assert_eq!(args.target.target.as_deref(), Some("art"));
            assert!(args.json);
        }
        _ => panic!("expected receipt command"),
    }
}

#[test]
fn sqlite_readonly_required_routes_to_signed_exec() {
    let error = Error::with_code(
        ErrorKind::Rpc,
        "sqlite_readonly_required",
        "database error (sqlite_readonly_required): use exec for state-changing SQL",
    );
    assert!(sqlite_requires_exec(&error));

    let error = Error::with_kind(
        ErrorKind::Rpc,
        "database error (sqlite_prepare_failed): no such table: missing",
    );
    assert!(!sqlite_requires_exec(&error));

    let error = Error::with_kind(
        ErrorKind::Rpc,
        "database error (sqlite_prepare_failed): detail mentions sqlite_readonly_required",
    );
    assert!(!sqlite_requires_exec(&error));
}

#[test]
fn script_detection_preserves_sqlite_read_vs_exec_boundary() {
    assert!(!looks_like_sql_script("select ';' as semi;"));
    assert!(!looks_like_sql_script("select /* ; */ 1;"));
    assert!(!looks_like_sql_script("select -- ;\n 1;"));
    assert!(!looks_like_sql_script("select `semi;name` from demo;"));
    assert!(!looks_like_sql_script("select [semi;name] from demo;"));
    assert!(!looks_like_sql_script("select 1; -- trailing comment"));
    assert!(!looks_like_sql_script("select 1; /* trailing comment */"));
    assert!(looks_like_sql_script(
        "create table person(first_name text); insert into person values ('Ada');"
    ));
    assert!(looks_like_sql_script("select 1; /* comment */ select 2;"));
}

#[test]
fn sql_script_splitter_respects_quotes_and_comments() {
    let statements = portability::split_sql_statements(
        "insert into t values ('semi;colon'); -- ; comment\ninsert into t values (\"two;semi\");",
    );
    assert_eq!(statements.len(), 2);
    assert!(statements[0].contains("'semi;colon'"));
    assert!(statements[1].contains("\"two;semi\""));
}

#[test]
fn sql_script_splitter_keeps_triggers_whole() {
    let statements = portability::split_sql_statements(
        "create table trigger_log(id integer);
create trigger log_person after insert on person begin
  insert into trigger_log values (new.id);
  select case when new.id > 0 then 'ok' else 'no' end;
end;
insert into person values (1);",
    );
    assert_eq!(statements.len(), 3);
    assert!(statements[0].starts_with("create table trigger_log"));
    assert!(statements[1].starts_with("create trigger log_person"));
    assert!(statements[1].contains("insert into trigger_log"));
    assert!(statements[1].contains("case when"));
    assert!(statements[2].starts_with("insert into person"));
}

#[test]
fn sql_script_splitter_handles_sqlite_dump_style_trigger_fixture() {
    let dump = "PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE artist(
  id integer primary key,
  name text not null
);
CREATE TABLE audit(
  artist_id integer not null,
  note text not null
);
CREATE TRIGGER artist_ai after insert on artist BEGIN
  INSERT INTO audit VALUES(new.id, 'created; yes');
  SELECT CASE WHEN new.name LIKE 'P%' THEN 'modern;ok' ELSE 'classic;ok' END;
END;
INSERT INTO artist VALUES(1,'Monet');
COMMIT;";
    let statements = portability::split_sql_statements(dump);
    assert_eq!(statements.len(), 7);
    assert!(portability::should_skip_import_wrapper(&statements[0]));
    assert!(portability::should_skip_import_wrapper(&statements[1]));
    assert!(statements[5].starts_with("INSERT INTO artist"));
    assert!(portability::should_skip_import_wrapper(&statements[6]));
    let trigger = &statements[4];
    assert!(trigger.starts_with("CREATE TRIGGER artist_ai"));
    assert!(trigger.contains("'created; yes'"));
    assert!(trigger.contains("'modern;ok'"));
    assert!(trigger.trim_end().ends_with("END;"));
}

#[test]
fn sqlite_dump_wrappers_are_skipped_for_octra_restore() {
    assert!(portability::should_skip_import_wrapper(
        "PRAGMA foreign_keys=OFF;"
    ));
    assert!(portability::should_skip_import_wrapper(
        "BEGIN TRANSACTION;"
    ));
    assert!(portability::should_skip_import_wrapper("COMMIT;"));
    assert!(!portability::should_skip_import_wrapper(
        "create table person(id integer);"
    ));
}

#[test]
fn small_sqlite_dump_restore_skips_shell_wrappers() {
    let statements = portability::split_sql_statements(
        "PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE person(id integer);
COMMIT;",
    );
    let script = portability::sql_script_for_single_exec(&statements);
    assert!(!script.contains("foreign_keys"));
    assert!(!script.contains("BEGIN TRANSACTION"));
    assert!(script.contains("CREATE TABLE person"));
    assert!(!script.contains("COMMIT"));
}

#[test]
fn dot_parser_handles_quotes_and_rejects_shell_pipe_forms() {
    assert_eq!(
        shell::parse_dot_parts(".backup main \"organization copy.sqlite\"").unwrap(),
        vec![".backup", "main", "organization copy.sqlite"]
    );
    assert!(shell::reject_shell_pipe_arg("|cat", ".read").is_err());
    assert!(
        shell::import_args(&[
            "--csv".to_string(),
            "--skip".to_string(),
            "1".to_string(),
            "person.csv".to_string(),
            "person".to_string(),
        ])
        .is_ok()
    );
    assert!(shell::import_args(&["person.csv".to_string(), "person".to_string()]).is_err());
}

#[test]
fn sqlite_dot_arguments_are_quoted_without_shell_escape() {
    assert_eq!(
        portability::sqlite_dot_argument("person").unwrap(),
        "person"
    );
    assert_eq!(
        portability::sqlite_dot_argument("person table").unwrap(),
        "'person table'"
    );
    assert_eq!(
        portability::sqlite_dot_argument("person-table").unwrap(),
        "person-table"
    );
    assert!(portability::sqlite_dot_argument("person'table").is_err());
}

#[test]
fn schema_dot_command_formats_sql_not_metadata_table() {
    let result = json!({
        "columns": ["type", "name", "sql"],
        "rows": [
            ["index", "sqlite_autoindex_collection_1", ""],
            ["table", "collection", "CREATE TABLE collection(\n  name text primary key\n)"]
        ]
    });
    let rendered = format_schema_result(&result).unwrap();
    assert_eq!(
        rendered,
        "CREATE TABLE collection(\n  name text primary key\n);\n"
    );
    assert!(!rendered.contains("sqlite_autoindex"));
    assert!(!rendered.contains("+---"));
}

#[test]
fn deploy_requires_explicit_unconfigured_escape_hatch() {
    let cli = Cli::try_parse_from(["octra-sqlite", "deploy", "--allow-unconfigured"]).unwrap();
    match cli.command {
        Commands::Deploy(args) => assert!(args.allow_unconfigured),
        _ => panic!("expected deploy command"),
    }
}

#[test]
fn deploy_requires_explicit_circle() {
    let args = DeployArgs {
        build: false,
        circle: None,
        wasm: None,
        ou: "200000".to_string(),
        rpc: None,
        no_wait: false,
        allow_unconfigured: false,
        bootstrap_owner: false,
        wallet: None,
        caller: None,
        private_key_b64: None,
        public_key_b64: None,
    };
    let error = cmd_deploy(args).unwrap_err().to_string();
    assert!(error.contains("requires --circle"));
}

#[test]
fn deploy_accepts_owner_bootstrap_for_explicit_circle() {
    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "deploy",
        "--circle",
        "oct://mainnet/octABC",
        "--bootstrap-owner",
    ])
    .unwrap();
    match cli.command {
        Commands::Deploy(args) => {
            assert_eq!(args.circle.as_deref(), Some("oct://mainnet/octABC"));
            assert!(args.bootstrap_owner);
            assert!(!args.allow_unconfigured);
        }
        _ => panic!("expected deploy command"),
    }
}

#[test]
fn upgrade_parses_apply_and_rollback_workflows() {
    let cli =
        Cli::try_parse_from(["octra-sqlite", "upgrade", "art", "--dry-run", "--json"]).unwrap();
    match cli.command {
        Commands::Upgrade(args) => {
            assert_eq!(args.target.as_deref(), Some("art"));
            assert!(args.dry_run);
            assert!(args.json);
            assert!(!args.unsafe_no_rollback);
            assert!(args.previous_wasm.is_none());
            assert!(args.rollback_bundle.is_none());
        }
        _ => panic!("expected upgrade command"),
    }

    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "upgrade",
        "art",
        "--unsafe-no-rollback",
        "--dry-run",
    ])
    .unwrap();
    match cli.command {
        Commands::Upgrade(args) => {
            assert!(args.unsafe_no_rollback);
        }
        _ => panic!("expected upgrade command"),
    }

    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "upgrade",
        "art",
        "--previous-wasm",
        "/tmp/old.wasm",
        "--dry-run",
    ])
    .unwrap();
    match cli.command {
        Commands::Upgrade(args) => {
            assert_eq!(
                args.previous_wasm.as_deref(),
                Some(Path::new("/tmp/old.wasm"))
            );
        }
        _ => panic!("expected upgrade command"),
    }

    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "upgrade",
        "art",
        "--write-smoke",
        "--write-ou",
        "200000",
    ])
    .unwrap();
    match cli.command {
        Commands::Upgrade(args) => {
            assert!(args.write_smoke);
            assert_eq!(args.write_ou.as_deref(), Some("200000"));
        }
        _ => panic!("expected upgrade command"),
    }

    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "upgrade",
        "rollback",
        "/tmp/octra-sqlite-upgrade",
        "--yes",
        "--json",
    ])
    .unwrap();
    match cli.command {
        Commands::Upgrade(args) => {
            assert_eq!(args.target.as_deref(), Some("rollback"));
            assert_eq!(
                args.rollback_bundle.as_deref(),
                Some(Path::new("/tmp/octra-sqlite-upgrade"))
            );
            assert!(args.yes);
            assert!(args.json);
        }
        _ => panic!("expected upgrade command"),
    }
}

#[test]
fn default_release_artifacts_are_embedded() {
    let artifact = resolve_bundled_wasm_artifact().unwrap();
    assert_eq!(artifact.source, format!("embedded:{DEFAULT_WASM_REL}"));
    assert_eq!(sha256_hex(&artifact.bytes), EXPECTED_WASM_SHA256);
    if env::var_os("OCTRA_SQLITE_MANIFEST").is_none() {
        let artifact = resolve_release_manifest().unwrap();
        assert_eq!(artifact.source, format!("embedded:{RELEASE_MANIFEST_REL}"));
        let manifest: Value = serde_json::from_str(&artifact.text).unwrap();
        assert_eq!(
            manifest["release"].as_str(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            EMBEDDED_WASM_BYTES
                .windows(OWNER_PUBKEY_PLACEHOLDER.len())
                .position(|window| window == OWNER_PUBKEY_PLACEHOLDER),
            manifest["personalization"]["owner_pubkey_offset"]
                .as_u64()
                .map(|offset| offset as usize)
        );
        assert_eq!(
            EMBEDDED_WASM_BYTES
                .windows(DB_ID_PLACEHOLDER.len())
                .position(|window| window == DB_ID_PLACEHOLDER),
            manifest["personalization"]["db_id_offset"]
                .as_u64()
                .map(|offset| offset as usize)
        );
        assert_eq!(
            manifest["storage"]["max_pages"].as_u64(),
            Some(MAX_DB_PAGES as u64)
        );
        assert_eq!(
            manifest["storage"]["max_file_bytes"].as_u64(),
            Some(MAX_DB_FILE_BYTES as u64)
        );
        assert_eq!(
            manifest["storage"]["max_dirty_pages_per_exec"].as_u64(),
            Some(MAX_DIRTY_PAGES_PER_EXEC as u64)
        );
        assert_eq!(
            manifest["execution"]["max_query_vdbe_steps"].as_u64(),
            Some(MAX_QUERY_VDBE_STEPS as u64)
        );
        assert_eq!(
            manifest["execution"]["max_exec_vdbe_steps"].as_u64(),
            Some(MAX_EXEC_VDBE_STEPS as u64)
        );
    }
}

#[test]
fn upgrade_without_database_enters_guided_workflow() {
    let cli = Cli::try_parse_from(["octra-sqlite", "upgrade"]).unwrap();
    match cli.command {
        Commands::Upgrade(args) => {
            assert!(args.target.is_none());
            assert!(args.rollback_bundle.is_none());
            assert!(!args.yes);
            assert!(!args.json);
        }
        _ => panic!("expected upgrade command"),
    }
}

#[test]
fn upgrade_bundle_label_uses_current_version_and_utc_date_only() {
    assert_eq!(upgrade::utc_date_label(0), "19700101",);
    assert_eq!(
        upgrade::upgrade_bundle_label("devnet", "octABC", "3.53.2", 0),
        "devnet-octABC-sqlite-3.53.2-19700101",
    );
    assert_eq!(
        upgrade::upgrade_bundle_label("dev/net", "oct ABC", "3.53.2", 0),
        "dev-net-oct-ABC-sqlite-3.53.2-19700101",
    );
}

#[test]
fn upgrade_recovers_wasm_from_nested_transaction_message() {
    let bytes = b"fake wasm";
    let hash = sha256_hex(bytes);
    let tx = json!({
        "tx_hash": "tx1",
        "message": serde_json::to_string(&json!({
            "code_b64": general_purpose::STANDARD.encode(bytes),
        })).unwrap(),
    });
    let recovered = upgrade::wasm_from_json(&tx, &hash).unwrap().unwrap();
    assert_eq!(recovered, bytes);
    assert_eq!(upgrade::find_tx_hash_with_code(&tx).as_deref(), Some("tx1"));
}

#[test]
fn restore_accepts_owner_bootstrap_for_explicit_uri() {
    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "restore",
        "oct://mainnet/octABC",
        "--file",
        "schema.sql",
        "--bootstrap-owner",
        "--verbose-sql",
        "--json-summary",
    ])
    .unwrap();
    match cli.command {
        Commands::Restore(args) => {
            assert_eq!(args.target.target.as_deref(), Some("oct://mainnet/octABC"));
            assert_eq!(args.file, Some(PathBuf::from("schema.sql")));
            assert!(args.bootstrap_owner);
            assert!(args.verbose_sql);
            assert!(args.json_summary);
        }
        _ => panic!("expected restore command"),
    }
}

#[test]
fn bootstrap_owner_only_accepts_empty_storage_cache_errors() {
    let zero_root = "0000000000000000000000000000000000000000000000000000000000000000";
    assert!(is_empty_storage_cache_error(&format!(
        "octra_circleViewAuth failed: missing storage cache: octABC:{zero_root}"
    )));
    assert!(!is_empty_storage_cache_error(
        "octra_circleViewAuth failed: missing storage cache: octABC:1111111111111111111111111111111111111111111111111111111111111111"
    ));
    assert!(!is_empty_storage_cache_error(
        "octra_circleViewAuth failed: wasm export returned 1"
    ));
}

#[test]
fn bootstrap_owner_json_marks_first_write_recovery() {
    let metadata = BootstrapOwnerMetadata {
        uri: "oct://mainnet/octABC".to_string(),
        owner: "octOwner".to_string(),
        owner_pubkey: "aa".repeat(32),
        db_id: "bb".repeat(32),
        code_hash: "cc".repeat(32),
    };
    let mode = BootstrapOwnerMode::FirstWrite(metadata);
    let value = add_bootstrap_owner_json(json!({"ok": true}), Some(&mode));
    assert_eq!(value["bootstrap_owner"], true);
    assert_eq!(value["bootstrap"]["mode"], "owner_first_write");
    assert_eq!(value["bootstrap"]["reason"], "empty_storage_cache");
    assert_eq!(value["bootstrap"]["uri"], "oct://mainnet/octABC");
}

#[test]
fn bootstrap_owner_json_marks_already_bootstrapped_restore() {
    let mode = BootstrapOwnerMode::AlreadyBootstrapped;
    let value = add_bootstrap_owner_json(json!({"ok": true}), Some(&mode));
    assert_eq!(value["bootstrap_owner"], true);
    assert_eq!(value["bootstrap"]["mode"], "normal_restore");
    assert_eq!(value["bootstrap"]["reason"], "already_bootstrapped");
}

#[test]
fn version_flag_reports_package_version() {
    let error = match Cli::try_parse_from(["octra-sqlite", "--version"]) {
        Ok(_) => panic!("expected version display"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    assert!(error.to_string().contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn new_accepts_sqlite_style_positional_sql() {
    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "new",
        "my-db",
        "create table people(first_name text);",
        "insert into people values ('Ada');",
    ])
    .unwrap();
    match cli.command {
        Commands::New(args) => {
            assert_eq!(args.name.as_deref(), Some("my-db"));
            assert_eq!(
                args.sql_args,
                vec![
                    "create table people(first_name text);",
                    "insert into people values ('Ada');"
                ]
            );
            assert_eq!(collect_initializer_sql(&args).unwrap(), args.sql_args);
            assert!(args.wasm.is_none());
        }
        _ => panic!("expected new command"),
    }
}

#[test]
fn open_accepts_read_rpc_trace_path() {
    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "open",
        "art",
        "--trace-rpc-json",
        "trace.jsonl",
        "--trace-rpc-json-mode",
        "summary",
        "select * from artist;",
    ])
    .unwrap();
    match cli.command {
        Commands::Open(args) => {
            assert_eq!(args.target.target.as_deref(), Some("art"));
            assert_eq!(args.trace_rpc_json, Some(PathBuf::from("trace.jsonl")));
            assert_eq!(args.trace_rpc_json_mode, TraceRpcJsonMode::Summary);
            assert_eq!(args.sql, vec!["select * from artist;"]);
        }
        _ => panic!("expected open command"),
    }
}

#[test]
fn trace_mode_requires_trace_path() {
    let args = OpenArgs {
        target: TargetArgs {
            target: Some("oct://devnet/octABC".to_string()),
            wallet: None,
            rpc: Some("mock://rpc".to_string()),
            caller: Some("octCaller".to_string()),
            private_key_b64: Some(
                "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
            ),
            public_key_b64: None,
        },
        json: false,
        trace_rpc_json: None,
        trace_rpc_json_mode: TraceRpcJsonMode::Summary,
        sql_file: None,
        read_only: false,
        ou: None,
        sql: vec!["select 1;".to_string()],
    };
    let error = cmd_open(args).unwrap_err().to_string();
    assert!(error.contains("--trace-rpc-json-mode requires --trace-rpc-json"));
}

#[test]
fn restore_summary_envelope_omits_per_batch_receipts() {
    let session = build_session(&TargetArgs {
        target: Some("oct://devnet/octABC".to_string()),
        wallet: None,
        rpc: Some("mock://rpc".to_string()),
        caller: Some("octCaller".to_string()),
        private_key_b64: Some(
            "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
        ),
        public_key_b64: None,
    })
    .unwrap();
    let plan = SqlScriptPlan {
        source_bytes: 42,
        total_statements: 2,
        executable_statements: 2,
        skipped_statements: 0,
        batches: 2,
        max_statement_bytes: 21,
        max_payload_bytes: 21,
    };
    let execution = SqlScriptExecution {
        statements: 2,
        batches: 2,
        results: vec![
            json!({"tx_hash": "tx1", "tx_url": "https://example/tx1", "receipt": {"success": true}}),
            json!({"tx_hash": "tx2", "tx_url": "https://example/tx2", "receipt": {"success": true}}),
        ],
    };
    let envelope = restore_summary_envelope(&session, &plan, &execution);
    assert_eq!(envelope["type"], "restore");
    assert_eq!(envelope["summary"], true);
    assert_eq!(envelope["writes"]["total"], 2);
    assert_eq!(envelope["writes"]["confirmed"], 2);
    assert_eq!(envelope["writes"]["first_tx_hash"], "tx1");
    assert_eq!(envelope["writes"]["last_tx_hash"], "tx2");
    assert!(envelope.get("progress").is_none());
}

#[test]
fn receipt_result_success_uses_sqlite_error_events() {
    let ok = json!({
        "circle": "octABC",
        "wallet": "octCaller",
        "tx_hash": "tx1",
        "result": {},
        "receipt": {"success": true, "error": null, "events": []}
    });
    assert!(receipt_result_success(&ok));

    let failed = json!({
        "circle": "octABC",
        "wallet": "octCaller",
        "tx_hash": "tx1",
        "result": {},
        "receipt": {
            "success": true,
            "error": null,
            "events": [{
                "event": "octra.sqlite.error",
                "values": ["sqlite_exec_failed: near bad: syntax error"]
            }]
        }
    });
    assert!(!receipt_result_success(&failed));
}

#[test]
fn receipt_target_validation_fails_closed() {
    let ok = json!({"contract": "octABC", "success": true});
    assert!(ensure_receipt_matches_circle(&ok, "octABC").is_ok());

    let mismatch =
        ensure_receipt_matches_circle(&json!({"contract": "octOTHER", "success": true}), "octABC")
            .unwrap_err();
    assert_eq!(error_code(&mismatch), "target_error");

    let missing = ensure_receipt_matches_circle(&json!({"success": true}), "octABC").unwrap_err();
    assert_eq!(error_code(&missing), "target_error");
}

#[test]
fn write_summary_treats_sqlite_error_event_as_rejected() {
    let summary = write_result_summary(&json!({
        "tx_hash": "tx1",
        "receipt": {
            "success": true,
            "error": null,
            "events": [{
                "event": "octra.sqlite.error",
                "values": ["sqlite_exec_failed: no such table"]
            }]
        }
    }));
    assert_eq!(summary["status"], "rejected");
}

#[test]
fn limits_json_exposes_automation_contract_facts() {
    let limits = limits_json(None);
    assert_eq!(limits["ok"], true);
    assert_eq!(limits["type"], "limits");
    assert_eq!(limits["schema"], "octra-sqlite.cli.v1");
    assert_eq!(limits["versions"]["sqlite"], SQLITE_VERSION);
    assert_eq!(limits["sql"]["max_sql_bytes"], MAX_SQL_TEXT_BYTES);
    assert_eq!(limits["result"]["max_rows"], MAX_RESULT_ROWS);
    assert_eq!(limits["storage"]["max_pages"], MAX_DB_PAGES);
    assert_eq!(limits["storage"]["max_file_bytes"], MAX_DB_FILE_BYTES);
    assert_eq!(
        limits["storage"]["max_dirty_pages_per_exec"],
        MAX_DIRTY_PAGES_PER_EXEC
    );
    assert_eq!(MAX_DB_PAGES * SQLITE_PAGE_BYTES, MAX_DB_FILE_BYTES);
    assert_eq!((STABLE_STORAGE_LIMIT_BYTES - 109) / 4_158, MAX_DB_PAGES);
    assert_eq!(
        limits["execution"]["max_query_vdbe_steps"],
        MAX_QUERY_VDBE_STEPS
    );
    assert_eq!(
        limits["execution"]["runtime_wasm_fuel"],
        "octra_protocol_dependency"
    );
    assert_eq!(limits["result"]["limit_error"], "result_limit_exceeded");
    assert_eq!(
        limits["auth"]["read_model"],
        "raw Circle targets detect the Octra read surface; sealed uses signed Octra view auth; public uses unsigned Octra circle view"
    );
    assert_eq!(limits["auth"]["read_modes"], json!(["sealed", "public"]));
    assert_eq!(
        limits["auth"]["read_mode_overrides"],
        json!(["auto", "sealed", "public"])
    );
    assert_eq!(limits["auth"]["write_model"], "OSW1 owner write intent");
    assert_eq!(limits["confidentiality"]["encrypted"], false);
    assert_eq!(
        limits["confidentiality"]["write_sql_visible_in_transaction_history"],
        true
    );
    assert!(
        limits["trace"]["modes"]
            .as_array()
            .unwrap()
            .contains(&json!("summary"))
    );
}

#[test]
fn commands_json_lists_public_cli_surface() {
    let commands = commands_json();
    assert_eq!(commands["ok"], true);
    assert_eq!(commands["type"], "commands");
    assert_eq!(commands["schema"], "octra-sqlite.cli.v1");
    assert!(
        commands["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| {
                command.get("command").and_then(Value::as_str)
                    == Some("octra-sqlite DATABASE \"SQL\"")
                    && command
                        .get("envelopes")
                        .and_then(Value::as_array)
                        .unwrap()
                        .contains(&json!("query"))
            })
    );
    assert!(
        commands["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command.get("command").and_then(Value::as_str)
                == Some("octra-sqlite database default NAME"))
    );
    assert!(
        commands["json_envelopes"]
            .as_array()
            .unwrap()
            .contains(&json!("new"))
    );
    assert!(
        commands["json_envelopes"]
            .as_array()
            .unwrap()
            .contains(&json!("upgrade"))
    );
    assert!(
        commands["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| {
                command.get("command").and_then(Value::as_str)
                    == Some("octra-sqlite upgrade DATABASE")
                    && command.get("envelope").and_then(Value::as_str) == Some("upgrade")
            })
    );
    assert!(
        !commands["json_envelopes"]
            .as_array()
            .unwrap()
            .contains(&json!("install"))
    );
    assert!(commands["discovery"].get("install").is_none());
    assert_eq!(
        commands["discovery"]["limits"],
        "octra-sqlite limits DATABASE --json"
    );
}

#[test]
fn commands_json_covers_every_public_top_level_command() {
    let commands = commands_json();
    let catalog = commands["commands"].as_array().unwrap();
    let catalog_names = catalog
        .iter()
        .filter_map(|command| command.get("command").and_then(Value::as_str))
        .filter_map(|command| command.split_whitespace().nth(1))
        .filter(|name| *name != "DATABASE")
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let clap_names = <Cli as clap::CommandFactory>::command()
        .get_subcommands()
        .map(|command| command.get_name().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        catalog_names, clap_names,
        "commands --json must cover every public top-level command"
    );
}

fn test_new_args(name: &str) -> NewArgs {
    NewArgs {
        name: Some(name.to_string()),
        build: false,
        wasm: None,
        create_ou: "200000".to_string(),
        ou: None,
        rpc: None,
        network: Some("devnet".to_string()),
        read_mode: ReadModeArg::Sealed,
        no_wait: false,
        no_name: false,
        default: false,
        sql: None,
        read: None,
        manifest: None,
        json: true,
        sample: None,
        wallet: None,
        caller: None,
        private_key_b64: None,
        public_key_b64: None,
        sql_args: Vec::new(),
    }
}

#[test]
fn new_manifest_uses_database_ontology() {
    let mut args = test_new_args("art");
    args.read = Some(PathBuf::from("schema.sql"));
    args.manifest = Some(PathBuf::from("art.json"));
    let created = CreatedCircle {
        circle: "octABC".to_string(),
        owner: "octOwner".to_string(),
        code_hash: "hash".to_string(),
        code_bytes: 123,
        auth_patch: AuthPatch {
            owner_pubkey_hex: "ownerpub".to_string(),
            db_id_hex: "dbid".to_string(),
            owner_pubkey_offset: 1,
            db_id_offset: 2,
        },
        tx_hash: Some("tx".to_string()),
        confirmation: None,
    };
    let init_sql = vec!["create table artist(id integer);".to_string()];
    let initializer_results = Vec::new();
    let manifest = new_manifest_json(NewManifestInput {
        args: &args,
        name: "art",
        target_uri: "oct://devnet/octABC",
        network: "devnet",
        created: &created,
        owner: "octOwner",
        rpc: "https://devnet.octrascan.io/rpc",
        init_sql: &init_sql,
        initializer_results: &initializer_results,
        readiness: json!({"checked": true, "ready": true}),
    });
    assert_eq!(manifest["manifest_version"], "octra-sqlite.database.v1");
    assert_eq!(manifest["database"]["name"], "art");
    assert_eq!(manifest["database"]["uri"], "oct://devnet/octABC");
    assert_eq!(manifest["database"]["read_uri"], "oct://devnet/octABC");
    assert_eq!(manifest["owner"]["write_auth"], "OSW1 owner write intent");
    assert_eq!(
        manifest["confidentiality"]["read_access"],
        "authenticated_wallet"
    );
    assert_eq!(manifest["confidentiality"]["read_owner_only"], false);
    assert_eq!(manifest["program"]["runtime"], "wasm_v1");
    assert_eq!(manifest["initializer"]["schema_file"], "schema.sql");
    assert!(manifest.get("app").is_none());
}

#[test]
fn public_database_read_uri_is_shareable() {
    assert_eq!(
        database_read_uri("oct://devnet/octABC", ReadMode::Public),
        "oct://devnet/octABC"
    );
    assert_eq!(
        database_read_uri("oct://devnet/octABC", ReadMode::Sealed),
        "oct://devnet/octABC"
    );
}

#[test]
fn new_refuses_to_overwrite_existing_database_name() {
    let args = test_new_args("art");
    let mut config = Config::default();
    config
        .databases
        .insert("art".to_string(), "oct://devnet/octABC".to_string());
    let error = ensure_new_database_name_available(&args, &config, "art").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("already exists"));
    assert!(message.contains("oct://devnet/octABC"));
}

#[test]
fn new_no_name_allows_existing_local_database_name() {
    let mut args = test_new_args("art");
    args.no_name = true;
    let mut config = Config::default();
    config
        .databases
        .insert("art".to_string(), "oct://devnet/octABC".to_string());
    ensure_new_database_name_available(&args, &config, "art").unwrap();
}

#[test]
fn new_accepts_builtin_sample() {
    let cli = Cli::try_parse_from(["octra-sqlite", "new", "my-db", "--sample", "artists"]).unwrap();
    match cli.command {
        Commands::New(args) => {
            assert_eq!(args.name.as_deref(), Some("my-db"));
            assert_eq!(args.sample.as_deref(), Some("artists"));
        }
        _ => panic!("expected new command"),
    }
}

#[test]
fn new_accepts_initializer_write_ou() {
    let cli = Cli::try_parse_from(["octra-sqlite", "new", "my-db", "--ou", "10000"]).unwrap();
    match cli.command {
        Commands::New(args) => {
            assert_eq!(args.name.as_deref(), Some("my-db"));
            assert_eq!(args.ou.as_deref(), Some("10000"));
            assert_eq!(
                resolve_new_initializer_write_ou(&args, &["create table t(id);".to_string()])
                    .unwrap()
                    .as_deref(),
                Some("10000")
            );
        }
        _ => panic!("expected new command"),
    }
}

#[test]
fn new_rejects_invalid_initializer_write_ou_before_creation() {
    let mut args = test_new_args("my-db");
    args.ou = Some("nope".to_string());
    let error =
        resolve_new_initializer_write_ou(&args, &["create table t(id);".to_string()]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("--ou must be a positive decimal integer"),
        "{error:#}"
    );
}

#[test]
fn new_without_initializer_does_not_need_write_ou() {
    let args = test_new_args("my-db");
    assert!(
        resolve_new_initializer_write_ou(&args, &[])
            .unwrap()
            .is_none()
    );
}

#[test]
fn new_accepts_public_read_mode() {
    let cli =
        Cli::try_parse_from(["octra-sqlite", "new", "my-db", "--read-mode", "public"]).unwrap();
    match cli.command {
        Commands::New(args) => {
            assert_eq!(args.name.as_deref(), Some("my-db"));
            assert_eq!(ReadMode::from(args.read_mode), ReadMode::Public);
        }
        _ => panic!("expected new command"),
    }
}

#[test]
fn new_accepts_wizard_mode_json_schema_and_manifest() {
    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "new",
        "--schema",
        "schema.sql",
        "--manifest",
        "database.json",
        "--json",
    ])
    .unwrap();
    match cli.command {
        Commands::New(args) => {
            assert!(args.name.is_none());
            assert_eq!(args.read.as_deref(), Some(Path::new("schema.sql")));
            assert_eq!(args.manifest.as_deref(), Some(Path::new("database.json")));
            assert!(args.json);
        }
        _ => panic!("expected new command"),
    }
}

#[test]
fn new_no_name_followup_uses_database_uri() {
    assert_eq!(
        new_followup_target("organization", "oct://devnet/octABC", true),
        "oct://devnet/octABC"
    );
    assert_eq!(
        new_followup_target("organization", "oct://devnet/octABC", false),
        "organization"
    );
}

#[test]
fn recovery_command_arguments_are_shell_safe() {
    assert_eq!(shell_quote("art"), "art");
    assert_eq!(shell_quote("my art"), "'my art'");
    assert_eq!(shell_quote("weird'name"), "'weird'\"'\"'name'");
    assert_eq!(dot_arg_quote("schema.sql"), "schema.sql");
    assert_eq!(
        dot_arg_quote("schema files/init.sql"),
        "\"schema files/init.sql\""
    );
    assert_eq!(dot_arg_quote("schema\"file.sql"), "\"schema\"\"file.sql\"");
}

#[test]
fn setup_accepts_noninteractive_defaults() {
    let cli = Cli::try_parse_from(["octra-sqlite", "setup", "--yes"]).unwrap();
    match cli.command {
        Commands::Setup(args) => assert!(args.yes),
        _ => panic!("expected setup command"),
    }
}

#[test]
fn setup_rejects_encrypted_oct_wallet_path() {
    let error = reject_encrypted_oct_wallet(Path::new("wallet.oct")).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("webcli .oct wallets are encrypted")
    );
    let error = reject_encrypted_oct_wallet(Path::new("wallet.OCT")).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("webcli .oct wallets are encrypted")
    );
}

#[test]
fn status_accepts_local_only_mode() {
    let cli = Cli::try_parse_from(["octra-sqlite", "status", "--skip-network"]).unwrap();
    match cli.command {
        Commands::Status(args) => assert!(args.skip_network),
        _ => panic!("expected status command"),
    }
}

#[test]
fn status_accepts_readiness_gate() {
    let cli = Cli::try_parse_from(["octra-sqlite", "status", "art", "--ready", "--json"]).unwrap();
    match cli.command {
        Commands::Status(args) => {
            assert_eq!(args.target.target.as_deref(), Some("art"));
            assert!(args.ready);
            assert!(args.json);
        }
        _ => panic!("expected status command"),
    }
}

#[test]
fn status_readiness_requires_all_database_items() {
    let mut report = StatusReport::new("status", true);
    report.init_database_readiness();
    assert!(!report.read_ready());
    assert!(!report.write_ready());
    for key in DATABASE_READINESS_KEYS {
        report.ready(key, true);
    }
    assert!(report.read_ready());
    assert!(report.write_ready());
    report.ready("sqlite_ready", false);
    assert!(!report.read_ready());
    assert!(report.write_ready());
    report.ready("owner_write_valid", false);
    assert!(!report.write_ready());
}

#[test]
fn status_tracks_upgrade_needed_separately_from_readiness() {
    let mut report = StatusReport::new("status", true);
    report.init_database_readiness();
    for key in DATABASE_READINESS_KEYS {
        report.ready(key, true);
    }
    report.engine_current(false);

    assert!(report.read_ready());
    assert!(report.write_ready());
    assert_eq!(report.engine_current, Some(false));
}

#[test]
fn status_json_promotes_stable_versions_and_upgrade_state() {
    let mut report = StatusReport::new("status", true);
    report.init_database_readiness();
    for key in DATABASE_READINESS_KEYS {
        report.ready(key, true);
    }
    report.sqlite_version("3.53.4");
    report.program_version("5");
    report.engine_current(true);

    let value = report.into_json_value(true, true, true, Some(false));

    assert_eq!(value["sqlite_version"], "3.53.4");
    assert_eq!(value["program_version"], "5");
    assert_eq!(value["engine_current"], true);
    assert_eq!(value["upgrade_needed"], false);
    assert_eq!(value["read_ready"], true);
    assert_eq!(value["write_ready"], true);
}

#[test]
fn status_version_fields_accept_only_strings() {
    assert_eq!(
        program_version_string(&json!({"version": "5"})),
        Some("5".to_string())
    );
    assert_eq!(program_version_string(&json!({"version": 5})), None);
    assert_eq!(program_version_string(&json!({"version": null})), None);

    assert_eq!(
        first_result_string(&json!({"rows": [["3.53.4"]]})),
        Some("3.53.4".to_string())
    );
    assert_eq!(first_result_string(&json!({"rows": [[3.53]]})), None);
    assert_eq!(first_result_string(&json!({"rows": []})), None);
}

#[test]
fn verify_write_smoke_json_reports_each_write_step() {
    let session = client_build_session(&ClientOptions {
        target: Some("oct://devnet/octABC?read_mode=public".to_string()),
        rpc: Some("mock://rpc".to_string()),
        caller: Some("octCurrent".to_string()),
        ..ClientOptions::default()
    })
    .unwrap();
    let confirmed = |hash: &str| {
        json!({
            "tx_hash": hash,
            "receipt": {
                "success": true,
                "error": null
            }
        })
    };
    let smoke = VerifyWriteSmoke {
        create: confirmed("create_tx"),
        insert: confirmed("insert_tx"),
        rows: json!({
            "columns": ["first_name", "last_name"],
            "rows": [["Ava", "North"]],
            "row_count": 1
        }),
        cleanup: confirmed("cleanup_tx"),
    };

    let envelope = verify_write_smoke_envelope(&session, smoke);

    assert_eq!(envelope["status"], "confirmed");
    assert_eq!(envelope["tx_hash"], "insert_tx");
    assert_eq!(envelope["statements"], 1);
    assert_eq!(envelope["create"]["status"], "confirmed");
    assert_eq!(envelope["create"]["tx_hash"], "create_tx");
    assert_eq!(envelope["cleanup"]["status"], "confirmed");
    assert_eq!(envelope["cleanup"]["tx_hash"], "cleanup_tx");
    assert_eq!(envelope["rows"]["row_count"], 1);
}

#[test]
fn historical_wasm_catalog_is_manifest_backed_metadata() {
    let manifest: Value = serde_json::from_str(EMBEDDED_RELEASE_MANIFEST).unwrap();
    let catalog = parse_historical_wasm_catalog(&manifest).unwrap();
    let entries = manifest["historical_wasm_catalog"].as_array().unwrap();
    assert_eq!(entries.len(), catalog.len());
    assert_eq!(
        manifest["upgrade_workflow"]["historical_wasm_catalog_mode"].as_str(),
        Some("metadata_only")
    );
    for (entry, artifact) in entries.iter().zip(&catalog) {
        assert!(matches!(
            artifact.sqlite_version.as_str(),
            "3.53.2" | "3.53.3"
        ));
        assert!(artifact.bytes > 0);
        assert_eq!(artifact.sha256.len(), 64);
        assert!(
            artifact
                .source_url
                .starts_with("https://raw.githubusercontent.com/")
        );
        assert_eq!(entry["releases"].as_str(), Some(artifact.releases.as_str()));
        assert_eq!(
            entry["sqlite_version"].as_str(),
            Some(artifact.sqlite_version.as_str())
        );
        assert_eq!(
            entry["source_url"].as_str(),
            Some(artifact.source_url.as_str())
        );
        assert_eq!(entry["bytes"].as_u64(), Some(artifact.bytes));
        assert_eq!(entry["sha256"].as_str(), Some(artifact.sha256.as_str()));
    }
    assert!(
        catalog.iter().any(|artifact| {
            artifact.releases == "0.6.0" && artifact.sqlite_version == "3.53.3"
        })
    );

    let exact = match_historical_wasm_in_catalog(&catalog, &catalog[2].sha256, None).unwrap();
    assert_eq!(exact.releases, "0.3.0");
    assert!(exact.exact_hash);

    let byte_match = match_historical_wasm_in_catalog(&catalog, "unknown", Some(609_475)).unwrap();
    assert_eq!(byte_match.releases, "0.3.0");
    assert!(!byte_match.exact_hash);
}

#[test]
fn wallet_status_accepts_target_and_json() {
    let cli = Cli::try_parse_from(["octra-sqlite", "wallet", "status", "art", "--json"]).unwrap();
    match cli.command {
        Commands::Wallet {
            command: WalletCommand::Status(args),
        } => {
            assert_eq!(args.target.target.as_deref(), Some("art"));
            assert!(args.json);
        }
        _ => panic!("expected wallet status command"),
    }
}

#[test]
fn wallet_attach_accepts_path_and_json() {
    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "wallet",
        "attach",
        "./wallet.json",
        "--json",
    ])
    .unwrap();
    match cli.command {
        Commands::Wallet {
            command: WalletCommand::Attach(args),
        } => {
            assert_eq!(args.path, PathBuf::from("./wallet.json"));
            assert!(args.json);
        }
        _ => panic!("expected wallet attach command"),
    }
}

#[test]
fn wallet_import_accepts_stdin_output_and_json() {
    let cli = Cli::try_parse_from([
        "octra-sqlite",
        "wallet",
        "import",
        "--stdin",
        "--output",
        "./wallet.json",
        "--json",
    ])
    .unwrap();
    match cli.command {
        Commands::Wallet {
            command: WalletCommand::Import(args),
        } => {
            assert!(args.stdin);
            assert_eq!(args.output.as_deref(), Some(Path::new("./wallet.json")));
            assert!(args.json);
        }
        _ => panic!("expected wallet import command"),
    }
}

#[cfg(unix)]
#[test]
fn wallet_permission_restriction_sets_owner_only_mode() {
    let path = std::env::temp_dir().join(format!(
        "octra-sqlite-wallet-perms-test-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, "{}").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    restrict_wallet_permissions_if_possible(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let _ = std::fs::remove_file(&path);
    assert_eq!(mode, 0o600);
}

#[test]
fn wallet_is_not_treated_as_database_shorthand() {
    let args = normalize_args(vec![
        "octra-sqlite".to_string(),
        "wallet".to_string(),
        "status".to_string(),
    ]);
    assert_eq!(args, vec!["octra-sqlite", "wallet", "status"]);
}

#[test]
fn remilia_sample_creates_expected_table() {
    let sql = sample_sql("artists").unwrap();
    assert!(sql.contains("create table artist"));
    assert!(sql.contains("Basquiat"));

    let sql = sample_sql("remilia").unwrap();
    assert!(sql.contains("create table collection"));
    assert!(sql.contains("Milady Maker"));
    assert!(!sql.contains("source_url"));
    assert!(!sql.contains("notes"));
    assert!(sample_sql("unknown").is_err());
}

#[test]
fn deploy_payload_json_matches_wasm_v1_circle_shape() {
    let payload = circle_deploy_payload_json(None, ReadMode::Sealed).unwrap();
    assert_eq!(
        payload,
        "{\"runtime\":\"wasm_v1\",\"privacy_class\":\"sealed\",\"browser_mode\":\"native_sealed\",\"resource_mode\":\"sealed_read\",\"code_b64\":null,\"policy_hash\":null,\"members_root\":null,\"export_policy\":null,\"limits\":{\"max_stable_bytes\":\"33554432\",\"max_assets_bytes\":\"33554432\",\"max_inline_value\":\"65536\",\"max_wasm_bytes\":\"33554432\"}}"
    );
}

#[test]
fn deploy_payload_json_supports_public_read_tuple() {
    let payload = circle_deploy_payload_json(None, ReadMode::Public).unwrap();
    assert_eq!(
        payload,
        "{\"runtime\":\"wasm_v1\",\"privacy_class\":\"public\",\"browser_mode\":\"gateway_allowed\",\"resource_mode\":\"public_resources\",\"code_b64\":null,\"policy_hash\":null,\"members_root\":null,\"export_policy\":null,\"limits\":{\"max_stable_bytes\":\"33554432\",\"max_assets_bytes\":\"33554432\",\"max_inline_value\":\"65536\",\"max_wasm_bytes\":\"33554432\"}}"
    );
}

#[test]
fn deploy_payload_json_can_inline_wasm_code() {
    let payload = circle_deploy_payload_json(Some("QUJD"), ReadMode::Sealed).unwrap();
    assert!(payload.contains("\"runtime\":\"wasm_v1\""));
    assert!(payload.contains("\"code_b64\":\"QUJD\""));
}

#[test]
fn deploy_confirmation_redacts_inline_wasm_code() {
    let redacted = redact_code_payload(json!({
        "message": "{\"code_b64\":\"QUJD\"}",
        "status": "confirmed"
    }));
    assert_eq!(redacted["message"], "{\"code_b64\":\"<redacted>\"}");
    assert_eq!(redacted["status"], "confirmed");
}
