use super::*;

pub(super) fn cmd_limits(args: LimitsArgs) -> Result<()> {
    let target = resolve_optional_target(&args.target)?;
    let limits = limits_json(target.clone());
    if args.json {
        return print_json(&limits);
    }
    print_field("max SQL bytes", MAX_SQL_TEXT_BYTES.to_string());
    print_field("batch target bytes", SQL_BATCH_TARGET_BYTES.to_string());
    print_field("max result rows", MAX_RESULT_ROWS.to_string());
    print_field("max database pages", MAX_DB_PAGES.to_string());
    print_field("max database bytes", MAX_DB_FILE_BYTES.to_string());
    print_field(
        "max dirty pages per exec",
        MAX_DIRTY_PAGES_PER_EXEC.to_string(),
    );
    print_field(
        "query work limit",
        format!("{MAX_QUERY_VDBE_STEPS} VDBE steps"),
    );
    print_field(
        "exec work limit",
        format!("{MAX_EXEC_VDBE_STEPS} VDBE steps"),
    );
    print_field("transactions", "one accepted exec is atomic");
    print_field("user BEGIN/COMMIT", "unsupported across Octra writes");
    print_field(
        "restore",
        "chunked; multi-batch restore can partially apply",
    );
    print_field(
        "sealed reads",
        "authenticated, not confidential or owner-only",
    );
    print_field("public reads", "walletless Octra circle view");
    print_field("writes", "OSW1 owner write intent");
    print_field(
        "write privacy",
        "SQL and values are visible in transaction history",
    );
    print_field("read-only", "client guard via --read-only");
    print_field("trace modes", "full, summary, request_only, response_meta");
    if let Some(target) = target {
        print_field("database", target["uri"].as_str().unwrap_or(""));
        print_field("network", target["network"].as_str().unwrap_or(""));
        print_field("circle", target["circle"].as_str().unwrap_or(""));
    }
    Ok(())
}

pub(super) fn cmd_commands(args: CommandsArgs) -> Result<()> {
    let commands = commands_json();
    if args.json {
        return print_json(&commands);
    }
    for command in commands
        .get("commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = command.get("command").and_then(Value::as_str).unwrap_or("");
        let purpose = command.get("purpose").and_then(Value::as_str).unwrap_or("");
        print_field(name, purpose);
    }
    Ok(())
}

pub(super) fn resolve_optional_target(args: &TargetArgs) -> Result<Option<Value>> {
    let explicit = args.target.is_some();
    let config = match load_config() {
        Ok(config) => config,
        Err(error) if explicit => return Err(error).context("loading config to resolve database"),
        Err(_) => return Ok(None),
    };
    let requested = args
        .target
        .clone()
        .or_else(|| config.default_database.clone())
        .or_else(|| env::var("OCTRA_SQLITE_DATABASE").ok())
        .or_else(|| env::var("OCTRA_SQLITE_TARGET").ok())
        .or_else(|| env::var("OCTRA_CIRCLE_ID").ok());
    let Some(requested) = requested else {
        return Ok(None);
    };
    let target = match resolve_target(&requested, &config) {
        Ok(target) => target,
        Err(error) if explicit => return Err(error).context("resolving database"),
        Err(_) => return Ok(None),
    };
    Ok(Some(json!({
        "requested": requested,
        "uri": canonical_database_uri(&target),
        "raw": target.raw,
        "network": target.network,
        "circle": target.circle,
        "rpc": target.rpc,
    })))
}

pub(super) fn limits_json(target: Option<Value>) -> Value {
    json!({
        "ok": true,
        "type": "limits",
        "schema": "octra-sqlite.cli.v1",
        "target": target,
        "versions": {
            "cli": env!("CARGO_PKG_VERSION"),
            "sqlite": SQLITE_VERSION,
            "json_schema": "octra-sqlite.cli.v1",
            "rpc_trace_schema": "octra-sqlite.rpc-trace.v1",
        },
        "sql": {
            "max_sql_bytes": MAX_SQL_TEXT_BYTES,
            "batch_target_bytes": SQL_BATCH_TARGET_BYTES,
            "input": ["argument", "stdin", "--sql-file", "--schema", ".read", "restore"],
        },
        "result": {
            "max_rows": MAX_RESULT_ROWS,
            "max_response_bytes": MAX_RESPONSE_BYTES,
            "limit_error": "result_limit_exceeded",
            "size_error": "result_too_large",
            "suggestion": "add a SQL LIMIT clause or narrow selected columns",
        },
        "storage": {
            "page_bytes": SQLITE_PAGE_BYTES,
            "max_pages": MAX_DB_PAGES,
            "max_file_bytes": MAX_DB_FILE_BYTES,
            "stable_storage_limit_bytes": STABLE_STORAGE_LIMIT_BYTES,
            "max_dirty_pages_per_exec": MAX_DIRTY_PAGES_PER_EXEC,
            "assumes_dedicated_circle_storage": true,
            "accounting": "Circle stable storage counts key and value bytes; the database limit reserves current VFS metadata and manifest overhead",
        },
        "execution": {
            "deterministic_sql_budget": true,
            "progress_interval_vdbe_steps": 1_000,
            "max_query_vdbe_steps": MAX_QUERY_VDBE_STEPS,
            "max_exec_vdbe_steps": MAX_EXEC_VDBE_STEPS,
            "query_error": "query_budget_exceeded",
            "exec_error": "exec_budget_exceeded",
            "runtime_wasm_fuel": "octra_protocol_dependency",
        },
        "restore": {
            "chunked": true,
            "json_summary": true,
            "progress": "batch_index, statement range, statement count, byte count",
            "retry_model": "make SQL idempotent; failed multi-batch restores can be rerun after inspection",
        },
        "transactions": {
            "exec_atomicity": "one accepted exec is atomic",
            "user_begin_commit": false,
            "multi_batch_atomic": false,
            "restore_partial_apply": true,
        },
        "auth": {
            "read_model": "raw Circle targets detect the Octra read surface; sealed uses signed Octra view auth; public uses unsigned Octra circle view",
            "read_modes": ["sealed", "public"],
            "read_mode_overrides": ["auto", "sealed", "public"],
            "sealed_reads": "octra_circleViewAuth",
            "public_reads": "octra_circleView",
            "write_model": "OSW1 owner write intent",
            "read_only_guard": "client-side --read-only",
            "native_roles": false,
        },
        "confidentiality": {
            "encrypted": false,
            "sealed_read_access": "authenticated_wallet",
            "sealed_owner_only": false,
            "write_sql_visible_in_transaction_history": true,
        },
        "trace": {
            "default": "off",
            "option": "--trace-rpc-json FILE",
            "modes": ["full", "summary", "request_only", "response_meta"],
            "mode_option": "--trace-rpc-json-mode MODE",
        }
    })
}

pub(super) fn commands_json() -> Value {
    json!({
        "ok": true,
        "type": "commands",
        "schema": "octra-sqlite.cli.v1",
        "versions": {
            "cli": env!("CARGO_PKG_VERSION"),
            "sqlite": SQLITE_VERSION,
            "json_schema": "octra-sqlite.cli.v1",
            "rpc_trace_schema": "octra-sqlite.rpc-trace.v1",
        },
        "commands": [
            {
                "command": "octra-sqlite setup",
                "purpose": "interactive wallet and network setup; can guide official wallet-generator import, attach, or masked private-key paste",
                "writes": false,
                "json": false,
            },
            {
                "command": "octra-sqlite new [DATABASE] [SQL]",
                "purpose": "create a Circle-backed SQLite database; prompts for wallet setup when needed and DATABASE is omitted in a terminal",
                "writes": true,
                "json": true,
                "envelope": "new",
            },
            {
                "command": "octra-sqlite new DATABASE --sample NAME",
                "purpose": "create a database from an explicit built-in sample",
                "writes": true,
                "json": true,
                "envelope": "new",
            },
            {
                "command": "octra-sqlite new DATABASE --read-mode public",
                "purpose": "create a public-read SQLite database; writes remain owner-signed",
                "writes": true,
                "json": true,
                "envelope": "new",
            },
            {
                "command": "octra-sqlite DATABASE \"SQL\"",
                "purpose": "run one SQL statement or script against a database; use --ou for owner-signed write budget",
                "writes": "depends_on_sql",
                "json": true,
                "envelopes": ["query", "write", "write_script"],
            },
            {
                "command": "octra-sqlite DATABASE --read-only \"SQL\"",
                "purpose": "run SQL while refusing state-changing statements",
                "writes": false,
                "json": true,
                "envelope": "query",
            },
            {
                "command": "octra-sqlite DATABASE --sql-file FILE",
                "purpose": "run SQL from a file",
                "writes": "depends_on_sql",
                "json": true,
                "envelopes": ["query", "write_script"],
            },
            {
                "command": "octra-sqlite open DATABASE",
                "purpose": "open the interactive sqlite> shell; use --ou for owner-signed write budget",
                "writes": "depends_on_sql",
                "json": false,
            },
            {
                "command": "octra-sqlite restore DATABASE --file dump.sql",
                "purpose": "restore large SQL text with chunked execution; use --ou for per-batch write budget",
                "writes": true,
                "json": true,
                "envelope": "restore",
            },
            {
                "command": "octra-sqlite check DATABASE --sql-file dump.sql",
                "purpose": "check script size and batching without writing",
                "writes": false,
                "json": true,
                "envelope": "check",
            },
            {
                "command": "octra-sqlite limits [DATABASE]",
                "purpose": "show SQL, restore, transaction, auth, and trace limits",
                "writes": false,
                "json": true,
                "envelope": "limits",
            },
            {
                "command": "octra-sqlite commands",
                "purpose": "show supported CLI commands and JSON envelopes",
                "writes": false,
                "json": true,
                "envelope": "commands",
            },
            {
                "command": "octra-sqlite status [DATABASE]",
                "purpose": "check config, wallet, WASM, Circle, auth, storage, and SQLite health",
                "writes": false,
                "json": true,
                "envelope": "status",
            },
            {
                "command": "octra-sqlite status [DATABASE] --ready",
                "purpose": "exit nonzero unless live read/query readiness checks pass",
                "writes": false,
                "json": true,
                "envelope": "status",
            },
            {
                "command": "octra-sqlite verify [DATABASE]",
                "purpose": "verify live Circle SQLite status and optional integrity/write checks; use --write-ou for write-smoke budget",
                "writes": "optional",
                "json": true,
                "envelope": "verify",
            },
            {
                "command": "octra-sqlite receipt TX_HASH [DATABASE]",
                "purpose": "wait for a submitted Circle transaction receipt without resubmitting the write",
                "writes": false,
                "json": true,
                "envelope": "receipt",
            },
            {
                "command": "octra-sqlite upgrade",
                "purpose": "guided backup, upgrade, and verify workflow for an existing database Circle",
                "writes": true,
                "json": false,
            },
            {
                "command": "octra-sqlite upgrade DATABASE",
                "purpose": "backup, upgrade, and verify an existing database Circle against the bundled SQLite engine; use --write-ou with --write-smoke",
                "writes": true,
                "json": true,
                "envelope": "upgrade",
            },
            {
                "command": "octra-sqlite upgrade DATABASE --dry-run",
                "purpose": "run owner, rollback, storage, and target-engine preflight without writing",
                "writes": false,
                "json": true,
                "envelope": "upgrade",
            },
            {
                "command": "octra-sqlite upgrade rollback BUNDLE",
                "purpose": "restore the previous verified Circle program from an upgrade bundle",
                "writes": true,
                "json": true,
                "envelope": "upgrade_rollback",
            },
            {
                "command": "octra-sqlite config",
                "purpose": "show local config, networks, RPC, explorer, and saved databases",
                "writes": false,
                "json": true,
            },
            {
                "command": "octra-sqlite database list",
                "purpose": "list saved database names",
                "writes": false,
                "json": true,
                "envelope": "database_list",
            },
            {
                "command": "octra-sqlite database info [DATABASE]",
                "purpose": "show database URI, Circle ID, network, and RPC",
                "writes": false,
                "json": true,
                "envelope": "database_info",
            },
            {
                "command": "octra-sqlite database set NAME URI",
                "purpose": "save an oct:// database URI locally",
                "writes": "local_config",
                "json": false,
            },
            {
                "command": "octra-sqlite database default NAME",
                "purpose": "set the default local database",
                "writes": "local_config",
                "json": false,
            },
            {
                "command": "octra-sqlite wallet status [DATABASE]",
                "purpose": "show wallet path, permissions, caller, and target read/write status",
                "writes": false,
                "json": true,
                "envelope": "wallet_status",
            },
            {
                "command": "octra-sqlite wallet attach PATH",
                "purpose": "make an existing plaintext wallet JSON the active wallet",
                "writes": "local_config",
                "json": true,
                "envelope": "wallet_attach",
            },
            {
                "command": "octra-sqlite wallet import PATH|--stdin",
                "purpose": "normalize a plaintext wallet JSON or stdin private key into a local wallet JSON",
                "writes": "local_file",
                "json": true,
                "envelope": "wallet_import",
            },
            {
                "command": "octra-sqlite deploy [OPTIONS]",
                "purpose": "update an existing Circle with Circle WASM",
                "writes": true,
                "json": true,
            },
        ],
        "json_envelopes": [
            "query",
            "new",
            "write",
            "write_script",
            "restore",
            "check",
            "limits",
            "commands",
            "status",
            "wallet_status",
            "wallet_attach",
            "wallet_import",
            "verify",
            "receipt",
            "upgrade",
            "upgrade_rollback",
            "database_list",
            "database_info",
            "error"
        ],
        "discovery": {
            "limits": "octra-sqlite limits DATABASE --json",
            "status": "octra-sqlite status DATABASE --json",
            "wallet": "octra-sqlite wallet status DATABASE --json",
            "json_docs": "docs/json-output.md",
        }
    })
}
