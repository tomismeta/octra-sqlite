use super::*;

pub(super) fn cmd_open(args: OpenArgs) -> Result<()> {
    if args.trace_rpc_json.is_none() && args.trace_rpc_json_mode != TraceRpcJsonMode::Full {
        bail!("--trace-rpc-json-mode requires --trace-rpc-json");
    }
    let session = build_session(&args.target)?;
    let trace_mode = RpcTraceMode::from(args.trace_rpc_json_mode);
    let trace_rpc_json = args
        .trace_rpc_json
        .as_deref()
        .map(|path| (path, trace_mode));
    let mode = if args.json {
        OutputMode::Json
    } else {
        OutputMode::Table
    };
    warn_wallet_load_error_for_public_reads(&session, mode);
    if let Some(path) = &args.sql_file {
        let sql = read_sql_file_arg(path)?;
        return run_sql_input(&session, &sql, mode, true, args.read_only, trace_rpc_json);
    }
    if args.sql.is_empty() {
        if let Some(sql) = read_stdin_sql()? {
            return run_sql_input(&session, &sql, mode, true, args.read_only, trace_rpc_json);
        }
        if args.trace_rpc_json.is_some() {
            bail!(
                "--trace-rpc-json requires one SQL statement; interactive shell tracing is not supported"
            );
        }
        run_shell(session, mode)
    } else {
        let sql = args.sql.join(" ");
        run_sql_input(&session, &sql, mode, true, args.read_only, trace_rpc_json)
    }
}

pub(super) fn warn_wallet_load_error_for_public_reads(session: &Session, mode: OutputMode) {
    if mode == OutputMode::Json {
        return;
    }
    let Some(error) = session.wallet_load_error() else {
        return;
    };
    eprintln!(
        "{}",
        format_status_line(
            "warn",
            "wallet",
            format!("failed to load; public reads can continue without it: {error}")
        )
    );
}

pub(super) fn cmd_restore(args: RestoreArgs) -> Result<()> {
    let session = build_session(&args.target)?;
    let bootstrap_owner = if args.bootstrap_owner {
        Some(resolve_bootstrap_owner_mode(&args.target, &session)?)
    } else {
        None
    };
    let sql = match args.file.as_deref() {
        Some(path) => read_sql_file_arg(path)?,
        None => read_stdin_sql()?.ok_or_else(|| anyhow!("restore requires --file or piped SQL"))?,
    };
    let plan = plan_sql_script(&sql)?;
    let json_output = args.json || args.json_summary;
    if !json_output {
        print_field("database", canonical_database_uri(session.target()));
        print_field("statements", plan.executable_statements.to_string());
        print_field("batches", plan.batches.to_string());
        if plan.skipped_statements > 0 {
            print_field(
                "skipped",
                format!("{} SQLite dump wrapper statements", plan.skipped_statements),
            );
        }
        if plan.batches > 1 {
            print_field(
                "atomicity",
                "each batch is atomic; the full restore can partially apply",
            );
        }
        match &bootstrap_owner {
            Some(BootstrapOwnerMode::FirstWrite(_)) => {
                print_field("bootstrap owner", "first batch only; OSW1 signed");
            }
            Some(BootstrapOwnerMode::AlreadyBootstrapped) => {
                print_field(
                    "bootstrap owner",
                    "already bootstrapped; running normal restore",
                );
            }
            None => {}
        }
    }
    let mut progress_events = Vec::new();
    let mut post_auth_error = None;
    let mut execution = if let Some(BootstrapOwnerMode::FirstWrite(metadata)) = &bootstrap_owner {
        let outcome = execute_sql_script_with_bootstrap_owner_progress(
            &session,
            &sql,
            &metadata.db_id,
            &metadata.owner_pubkey,
            args.verbose_sql,
            |progress| {
                if !json_output {
                    print_field("restore", format_progress(&progress));
                }
                if args.json {
                    progress_events.push(progress);
                }
            },
        )?;
        post_auth_error = outcome.post_auth_error;
        outcome.execution
    } else {
        let auth = auth_info(&session).context("reading owner auth for restore")?;
        let owner_pubkey = auth
            .owner_pubkey
            .as_deref()
            .ok_or_else(|| anyhow!("auth_info missing owner_pubkey"))?;
        execute_sql_script_with_owner_auth_progress(
            &session,
            &sql,
            &auth.db_id,
            owner_pubkey,
            args.verbose_sql,
            |progress| {
                if !json_output {
                    print_field("restore", format_progress(&progress));
                }
                if args.json {
                    progress_events.push(progress);
                }
            },
        )?
    };
    for result in &mut execution.results {
        let raw = std::mem::take(result);
        *result = with_explorer(raw, &session);
    }
    if let Some(error) = post_auth_error {
        return report_bootstrap_post_auth_failure(BootstrapPostAuthReport {
            session: &session,
            plan: &plan,
            execution: &execution,
            progress: &progress_events,
            mode: bootstrap_owner.as_ref(),
            json_summary: args.json_summary,
            json_full: args.json,
            post_auth_error: &error,
        });
    }
    if args.json_summary {
        print_json(&add_bootstrap_owner_json(
            restore_summary_envelope(&session, &plan, &execution),
            bootstrap_owner.as_ref(),
        ))
    } else if args.json {
        print_json(&add_bootstrap_owner_json(
            restore_envelope(&session, &plan, &execution, &progress_events),
            bootstrap_owner.as_ref(),
        ))
    } else {
        print_field(
            "complete",
            format!(
                "{} statements in {} batches",
                execution.statements, execution.batches
            ),
        );
        Ok(())
    }
}

pub(super) struct BootstrapPostAuthReport<'a> {
    session: &'a Session,
    plan: &'a SqlScriptPlan,
    execution: &'a SqlScriptExecution,
    progress: &'a [SqlBatchProgress],
    mode: Option<&'a BootstrapOwnerMode>,
    json_summary: bool,
    json_full: bool,
    post_auth_error: &'a str,
}

pub(super) fn report_bootstrap_post_auth_failure(
    report: BootstrapPostAuthReport<'_>,
) -> Result<()> {
    let first_write = report
        .execution
        .results
        .first()
        .map(write_result_summary)
        .unwrap_or_else(|| json!({"status": "missing"}));
    if report.json_summary || report.json_full {
        let base = if report.json_summary {
            restore_summary_envelope(report.session, report.plan, report.execution)
        } else {
            restore_envelope(
                report.session,
                report.plan,
                report.execution,
                report.progress,
            )
        };
        let mut envelope = add_bootstrap_owner_json(base, report.mode);
        if let Some(object) = envelope.as_object_mut() {
            object.insert("ok".to_string(), Value::Bool(false));
            object.insert(
                "status".to_string(),
                Value::String("bootstrap_post_auth_failed".to_string()),
            );
            object.insert(
                "post_auth_info".to_string(),
                json!({
                    "ok": false,
                    "error": report.post_auth_error,
                }),
            );
            object.insert("first_write".to_string(), first_write.clone());
        }
        print_json(&envelope)?;
    } else {
        print_field("bootstrap first write", value_to_string(&first_write));
        print_field("post auth_info", "failed");
        print_field("auth_info error", report.post_auth_error);
    }
    Err(coded_error(
        "bootstrap_unverified",
        format!(
            "bootstrap first write was submitted but post-write auth_info still failed; first_write={}; post_auth_info_error={post_auth_error}",
            serde_json::to_string(&first_write)?,
            post_auth_error = report.post_auth_error
        ),
    ))
}

#[derive(Clone, Debug)]
pub(super) enum BootstrapOwnerMode {
    FirstWrite(BootstrapOwnerMetadata),
    AlreadyBootstrapped,
}

#[derive(Clone, Debug)]
pub(super) struct BootstrapOwnerMetadata {
    pub(super) uri: String,
    pub(super) owner: String,
    pub(super) owner_pubkey: String,
    pub(super) db_id: String,
    pub(super) code_hash: String,
}

pub(super) fn resolve_bootstrap_owner_mode(
    target_args: &TargetArgs,
    session: &Session,
) -> Result<BootstrapOwnerMode> {
    let requested = target_args.target.as_deref().ok_or_else(|| {
        anyhow!("--bootstrap-owner requires an explicit oct://NETWORK/CIRCLE database URI")
    })?;
    if !requested.starts_with("oct://") {
        bail!("--bootstrap-owner requires an explicit oct://NETWORK/CIRCLE database URI");
    }

    match auth_info(session) {
        Ok(_) => return Ok(BootstrapOwnerMode::AlreadyBootstrapped),
        Err(error) if is_empty_storage_cache_error(&error.to_string()) => {}
        Err(error) => bail!(
            "--bootstrap-owner only handles empty storage-cache auth_info failures; auth_info failed with: {error:#}"
        ),
    }

    let metadata = find_bootstrap_owner_metadata(session)?;
    if metadata.owner != session.caller() {
        bail!(
            "bootstrap metadata owner {} does not match current wallet {}",
            metadata.owner,
            session.caller()
        );
    }
    let wallet_owner_pubkey = hex::encode(session.intent_public_key()?);
    if metadata.owner_pubkey != wallet_owner_pubkey {
        bail!("bootstrap metadata owner public key does not match the active wallet");
    }
    let expected_code_hash = bootstrap_owner_personalized_hash(&metadata)?;
    if metadata.code_hash != expected_code_hash {
        bail!(
            "bootstrap metadata code hash {} does not match locally personalized bundled WASM {expected_code_hash}",
            metadata.code_hash
        );
    }

    let info = program_info(session).context("reading Circle program info for bootstrap-owner")?;
    match program_owner(&info) {
        Some(owner) if owner == session.caller() => {}
        Some(owner) => bail!(
            "Circle owner is {owner}; current wallet {} cannot bootstrap owner writes",
            session.caller()
        ),
        None => bail!("Circle program info did not expose an owner; refusing bootstrap-owner"),
    }
    let live_code_hash = info
        .get("code_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Circle program info missing code_hash"))?;
    if live_code_hash != expected_code_hash {
        bail!(
            "live program hash {live_code_hash} does not match locally personalized bundled WASM {expected_code_hash}"
        );
    }

    Ok(BootstrapOwnerMode::FirstWrite(metadata))
}

pub(super) fn find_bootstrap_owner_metadata(session: &Session) -> Result<BootstrapOwnerMetadata> {
    let config = load_config()?;
    let uri = canonical_database_uri(session.target());
    let metadata = config
        .database_metadata
        .values()
        .find(|metadata| {
            metadata.uri == uri
                || (metadata.network == session.target().network
                    && metadata.circle == session.target().circle)
        })
        .ok_or_else(|| {
            anyhow!(
                "missing bootstrap metadata for {uri}; rerun deploy --bootstrap-owner with this CLI"
            )
        })?;
    Ok(BootstrapOwnerMetadata {
        uri,
        owner: metadata.owner.clone(),
        owner_pubkey: metadata.owner_pubkey.clone(),
        db_id: metadata.db_id.clone(),
        code_hash: metadata.code_hash.clone(),
    })
}

pub(super) fn bootstrap_owner_personalized_hash(
    metadata: &BootstrapOwnerMetadata,
) -> Result<String> {
    let artifact = resolve_bundled_wasm_artifact()?;
    let mut wasm = artifact.bytes;
    let owner_pubkey = hex_to_32("owner_pubkey", &metadata.owner_pubkey)?;
    let db_id = hex_to_32("db_id", &metadata.db_id)?;
    patch_wasm_auth_bytes(&mut wasm, &owner_pubkey, &db_id)?;
    Ok(sha256_hex(&wasm))
}

pub(super) fn is_empty_storage_cache_error(text: &str) -> bool {
    const ZERO_ROOT: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    text.contains("missing storage cache") && text.contains(ZERO_ROOT)
}

pub(super) fn add_bootstrap_owner_json(
    mut value: Value,
    mode: Option<&BootstrapOwnerMode>,
) -> Value {
    let Some(mode) = mode else {
        return value;
    };
    if let Some(object) = value.as_object_mut() {
        object.insert("bootstrap_owner".to_string(), Value::Bool(true));
        let bootstrap = match mode {
            BootstrapOwnerMode::FirstWrite(metadata) => json!({
                "mode": "owner_first_write",
                "reason": "empty_storage_cache",
                "uri": metadata.uri,
                "owner": metadata.owner,
                "owner_pubkey": metadata.owner_pubkey,
                "db_id": metadata.db_id,
                "code_hash": metadata.code_hash,
            }),
            BootstrapOwnerMode::AlreadyBootstrapped => json!({
                "mode": "normal_restore",
                "reason": "already_bootstrapped",
            }),
        };
        object.insert("bootstrap".to_string(), bootstrap);
    }
    value
}

pub(super) fn cmd_check(args: CheckArgs) -> Result<()> {
    let sql = collect_check_sql(&args)?;
    let plan = plan_sql_script(&sql)?;
    let target = resolve_optional_target(&args.target)?;
    let warnings = script_plan_warnings(&plan);
    if args.json {
        return print_json(&json!({
            "ok": true,
            "type": "check",
            "schema": "octra-sqlite.cli.v1",
            "syntax_checked": false,
            "target": target,
            "plan": script_plan_json(&plan),
            "warnings": warnings,
        }));
    }
    print_field("check", "ok");
    print_field("syntax", "not checked; SQLite validates SQL when run");
    if let Some(target) = target {
        print_field("database", target["uri"].as_str().unwrap_or(""));
    }
    print_field("statements", plan.executable_statements.to_string());
    print_field("batches", plan.batches.to_string());
    print_field("max statement bytes", plan.max_statement_bytes.to_string());
    for warning in warnings {
        print_field("warning", warning);
    }
    Ok(())
}

pub(super) fn read_sql_file_arg(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let sql = read_stdin_sql()?.ok_or_else(|| anyhow!("stdin did not contain SQL"))?;
        return Ok(sql);
    }
    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

pub(super) fn collect_check_sql(args: &CheckArgs) -> Result<String> {
    if let Some(path) = &args.sql_file {
        return read_sql_file_arg(path);
    }
    if let Some(sql) = &args.sql {
        return Ok(sql.clone());
    }
    read_stdin_sql()?.ok_or_else(|| anyhow!("check requires --sql, --sql-file, or piped SQL"))
}

pub(super) fn read_stdin_sql() -> Result<Option<String>> {
    if io::stdin().is_terminal() {
        return Ok(None);
    }
    let mut sql = String::new();
    io::stdin().read_to_string(&mut sql)?;
    if sql.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(sql))
    }
}

pub(super) fn run_sql_input(
    session: &Session,
    sql: &str,
    mode: OutputMode,
    headers: bool,
    read_only: bool,
    trace_rpc_json: Option<(&Path, RpcTraceMode)>,
) -> Result<()> {
    run_one_sql_to(session, sql, mode, headers, None, read_only, trace_rpc_json)
}

pub(super) fn run_one_sql_to(
    session: &Session,
    sql: &str,
    mode: OutputMode,
    headers: bool,
    output: Option<&Path>,
    read_only: bool,
    trace_rpc_json: Option<(&Path, RpcTraceMode)>,
) -> Result<()> {
    let trimmed = sql.trim();
    if trimmed.starts_with('.') && !trimmed.contains('\n') {
        if trace_rpc_json.is_some() {
            bail!("--trace-rpc-json supports one read-only SQL statement, not dot commands");
        }
        if read_only && write_dot_command(trimmed) {
            return Err(coded_error(
                "read_only",
                "read_only: dot command may write to the database",
            ));
        }
        run_dot_command(session.clone(), mode, headers, output, trimmed)?;
        return Ok(());
    }
    if looks_like_sql_script(sql) {
        if trace_rpc_json.is_some() {
            bail!("--trace-rpc-json supports one read-only SQL statement, not SQL scripts");
        }
        if read_only {
            return Err(coded_error(
                "read_only",
                "read_only: multi-statement SQL scripts are not submitted in read-only mode",
            ));
        }
        return run_exec_script_to(session, sql, mode, output);
    }
    ensure_sql_text_fits(sql)?;
    let query_result = match trace_rpc_json {
        Some((path, mode)) => query_typed_traced(session, sql, path, mode),
        None => Database::from_session(session.clone()).query_value(sql),
    };
    match query_result {
        Ok(result) => {
            if mode == OutputMode::Json {
                write_text(output, &format_json(&query_envelope(session, result))?)
            } else {
                write_text(output, &format_result(&result, mode, headers)?)
            }
        }
        Err(error) if sqlite_requires_exec(&error) => {
            if trace_rpc_json.is_some() {
                bail!("--trace-rpc-json is read-only; SQL would write");
            }
            if read_only {
                return Err(coded_error(
                    "read_only",
                    "read_only: SQL would write; remove --read-only to sign and submit it",
                ));
            }
            run_exec_sql_to(session, sql, mode, output)
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn run_exec_sql_to(
    session: &Session,
    sql: &str,
    mode: OutputMode,
    output: Option<&Path>,
) -> Result<()> {
    let result = with_explorer(
        Database::from_session(session.clone()).execute_value(sql, false)?,
        session,
    );
    if mode == OutputMode::Json {
        write_text(
            output,
            &format_json(&write_envelope(session, result, None))?,
        )
    } else {
        write_text(output, &format_exec_result(&result)?)
    }
}

pub(super) fn run_exec_script_to(
    session: &Session,
    sql: &str,
    mode: OutputMode,
    output: Option<&Path>,
) -> Result<()> {
    let plan = plan_sql_script(sql)?;
    let mut progress_events = Vec::new();
    let mut execution = execute_sql_script_with_progress(session, sql, false, |progress| {
        progress_events.push(progress);
    })?;
    for result in &mut execution.results {
        let raw = std::mem::take(result);
        *result = with_explorer(raw, session);
    }
    if mode == OutputMode::Json {
        write_text(
            output,
            &format_json(&script_envelope(
                "write_script",
                session,
                &plan,
                &execution,
                &progress_events,
            ))?,
        )
    } else {
        let mut rendered = String::new();
        for result in &execution.results {
            rendered.push_str(&format_exec_result(result)?);
        }
        rendered.push_str(&format!(
            "{} {} statements in {} batches\n",
            dim("script:"),
            execution.statements,
            execution.batches
        ));
        write_text(output, &rendered)
    }
}

pub(super) fn write_dot_command(command: &str) -> bool {
    let name = command
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(name.as_str(), ".read" | ".import")
}

pub(super) fn format_progress(progress: &SqlBatchProgress) -> String {
    format!(
        "batch {}/{} statements {}..{} ({} statements, {} bytes)",
        progress.batch_index,
        progress.total_batches,
        progress.start_statement,
        progress.end_statement,
        progress.statements,
        progress.bytes
    )
}

pub(super) fn query_envelope(session: &Session, result: Value) -> Value {
    json!({
        "ok": true,
        "type": "query",
        "schema": "octra-sqlite.cli.v1",
        "database": database_identity(session),
        "columns": result.get("columns").cloned().unwrap_or_else(|| json!([])),
        "rows": result.get("rows").cloned().unwrap_or_else(|| json!([])),
        "row_count": result.get("row_count").cloned().unwrap_or_else(|| {
            result
                .get("rows")
                .and_then(Value::as_array)
                .map(|rows| json!(rows.len()))
                .unwrap_or_else(|| json!(0))
        }),
        "result": result,
    })
}

pub(super) fn write_envelope(session: &Session, result: Value, statements: Option<usize>) -> Value {
    let summary = write_result_summary(&result);
    json!({
        "ok": true,
        "type": "write",
        "schema": "octra-sqlite.cli.v1",
        "database": database_identity(session),
        "status": summary["status"].clone(),
        "tx_hash": summary["tx_hash"].clone(),
        "statements": statements,
        "cost": summary["cost"].clone(),
        "receipt": result.get("receipt").cloned().unwrap_or(Value::Null),
        "result": result,
    })
}

pub(super) fn restore_envelope(
    session: &Session,
    plan: &SqlScriptPlan,
    execution: &SqlScriptExecution,
    progress: &[SqlBatchProgress],
) -> Value {
    script_envelope("restore", session, plan, execution, progress)
}

pub(super) fn restore_summary_envelope(
    session: &Session,
    plan: &SqlScriptPlan,
    execution: &SqlScriptExecution,
) -> Value {
    let writes = execution
        .results
        .iter()
        .map(write_result_summary)
        .collect::<Vec<_>>();
    let failed = writes
        .iter()
        .filter(|write| write.get("status").and_then(Value::as_str) == Some("rejected"))
        .cloned()
        .collect::<Vec<_>>();
    json!({
        "ok": true,
        "type": "restore",
        "schema": "octra-sqlite.cli.v1",
        "summary": true,
        "database": database_identity(session),
        "plan": script_plan_json(plan),
        "statements": execution.statements,
        "batches": execution.batches,
        "writes": {
            "total": writes.len(),
            "confirmed": writes.iter().filter(|write| write.get("status").and_then(Value::as_str) == Some("confirmed")).count(),
            "submitted": writes.iter().filter(|write| write.get("status").and_then(Value::as_str) == Some("submitted")).count(),
            "rejected": failed.len(),
            "first_tx_hash": writes.first().and_then(|write| write.get("tx_hash")).cloned().unwrap_or(Value::Null),
            "last_tx_hash": writes.last().and_then(|write| write.get("tx_hash")).cloned().unwrap_or(Value::Null),
            "first_tx_url": writes.first().and_then(|write| write.get("tx_url")).cloned().unwrap_or(Value::Null),
            "last_tx_url": writes.last().and_then(|write| write.get("tx_url")).cloned().unwrap_or(Value::Null),
            "failed": failed,
        }
    })
}

pub(super) fn script_envelope(
    envelope_type: &str,
    session: &Session,
    plan: &SqlScriptPlan,
    execution: &SqlScriptExecution,
    progress: &[SqlBatchProgress],
) -> Value {
    json!({
        "ok": true,
        "type": envelope_type,
        "schema": "octra-sqlite.cli.v1",
        "database": database_identity(session),
        "plan": script_plan_json(plan),
        "statements": execution.statements,
        "batches": execution.batches,
        "progress": progress.iter().map(progress_json).collect::<Vec<_>>(),
        "writes": execution
            .results
            .iter()
            .map(write_result_summary)
            .collect::<Vec<_>>(),
    })
}

pub(super) fn progress_json(progress: &SqlBatchProgress) -> Value {
    json!({
        "batch_index": progress.batch_index,
        "total_batches": progress.total_batches,
        "start_statement": progress.start_statement,
        "end_statement": progress.end_statement,
        "statements": progress.statements,
        "bytes": progress.bytes,
    })
}

pub(super) fn write_result_summary(result: &Value) -> Value {
    let receipt = result.get("receipt");
    let success = receipt
        .and_then(|receipt| receipt.get("success"))
        .and_then(Value::as_bool);
    let status = match success {
        Some(true) => "confirmed",
        Some(false) => "rejected",
        None => result
            .pointer("/result/status")
            .and_then(Value::as_str)
            .unwrap_or("submitted"),
    };
    json!({
        "status": status,
        "tx_hash": result.get("tx_hash").cloned().unwrap_or(Value::Null),
        "tx_url": result.get("tx_url").cloned().unwrap_or(Value::Null),
        "circle_url": result.get("circle_url").cloned().unwrap_or(Value::Null),
        "cost": {
            "ou": result.pointer("/result/ou_cost").cloned().unwrap_or(Value::Null),
            "effort": receipt
                .and_then(|receipt| receipt.get("effort"))
                .cloned()
                .unwrap_or(Value::Null),
        }
    })
}

pub(super) fn script_plan_json(plan: &SqlScriptPlan) -> Value {
    json!({
        "source_bytes": plan.source_bytes,
        "total_statements": plan.total_statements,
        "executable_statements": plan.executable_statements,
        "skipped_statements": plan.skipped_statements,
        "batches": plan.batches,
        "max_statement_bytes": plan.max_statement_bytes,
        "max_payload_bytes": plan.max_payload_bytes,
        "max_sql_bytes": MAX_SQL_TEXT_BYTES,
        "batch_target_bytes": SQL_BATCH_TARGET_BYTES,
    })
}

pub(super) fn script_plan_warnings(plan: &SqlScriptPlan) -> Vec<String> {
    let mut warnings = Vec::new();
    if plan.skipped_statements > 0 {
        warnings.push(format!(
            "{} SQLite dump wrapper statements will be skipped",
            plan.skipped_statements
        ));
    }
    if plan.batches > 1 {
        warnings.push("multi-batch restore can partially apply; make SQL idempotent".to_string());
    }
    if plan.skipped_statements > 0 {
        warnings.push("SQLite dump transaction wrappers are stripped before restore".to_string());
    }
    warnings
}

pub(super) fn database_identity(session: &Session) -> Value {
    let target = session.target();
    json!({
        "uri": canonical_database_uri(target),
        "raw": &target.raw,
        "network": &target.network,
        "circle": &target.circle,
        "rpc": &target.rpc,
        "wallet": session.caller(),
        "read_mode": target.read_mode.as_str(),
    })
}

pub(super) fn canonical_database_uri(target: &Target) -> String {
    format!("oct://{}/{}", target.network, target.circle)
}

pub(super) fn format_schema_result(result: &Value) -> Result<String> {
    let columns = result
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("schema result missing columns"))?;
    let sql_idx = columns
        .iter()
        .position(|column| column.as_str() == Some("sql"))
        .ok_or_else(|| anyhow!("schema result missing sql column"))?;
    let rows = result
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("schema result missing rows"))?;
    let mut out = String::new();
    for row in rows.iter().filter_map(Value::as_array) {
        let Some(sql) = row.get(sql_idx).map(value_to_string) else {
            continue;
        };
        let sql = sql.trim();
        if sql.is_empty() {
            continue;
        }
        out.push_str(sql);
        if !sql.ends_with(';') {
            out.push(';');
        }
        out.push('\n');
    }
    Ok(out)
}

pub(super) fn sqlite_requires_exec(error: &Error) -> bool {
    error.kind() == ErrorKind::Rpc && error.code() == Some("sqlite_readonly_required")
}

pub(super) fn looks_like_sql_script(sql: &str) -> bool {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backtick = false;
    let mut in_bracket = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut chars = sql.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if in_line_comment {
            if ch == '\n' || ch == '\r' {
                in_line_comment = false;
            }
            continue;
        }
        if in_block_comment {
            if ch == '*' && chars.peek().is_some_and(|(_, next)| *next == '/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }
        if in_bracket {
            if ch == ']' {
                if chars.peek().is_some_and(|(_, next)| *next == ']') {
                    chars.next();
                } else {
                    in_bracket = false;
                }
            }
            continue;
        }
        if in_backtick {
            if ch == '`' {
                if chars.peek().is_some_and(|(_, next)| *next == '`') {
                    chars.next();
                } else {
                    in_backtick = false;
                }
            }
            continue;
        }
        match ch {
            '\'' if !in_double_quote => {
                if in_single_quote && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    in_single_quote = !in_single_quote;
                }
            }
            '"' if !in_single_quote => {
                if in_double_quote && chars.peek().is_some_and(|(_, next)| *next == '"') {
                    chars.next();
                } else {
                    in_double_quote = !in_double_quote;
                }
            }
            '`' if !in_single_quote && !in_double_quote => in_backtick = true,
            '[' if !in_single_quote && !in_double_quote => in_bracket = true,
            '-' if !in_single_quote
                && !in_double_quote
                && chars.peek().is_some_and(|(_, next)| *next == '-') =>
            {
                chars.next();
                in_line_comment = true;
            }
            '/' if !in_single_quote
                && !in_double_quote
                && chars.peek().is_some_and(|(_, next)| *next == '*') =>
            {
                chars.next();
                in_block_comment = true;
            }
            ';' if !in_single_quote && !in_double_quote => {
                let rest = &sql[index + ch.len_utf8()..];
                if sql_tail_has_statement(rest) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

pub(super) fn sql_tail_has_statement(mut tail: &str) -> bool {
    loop {
        tail = tail.trim_start();
        if tail.is_empty() {
            return false;
        }
        if let Some(rest) = tail.strip_prefix("--") {
            tail = rest.split_once('\n').map(|(_, after)| after).unwrap_or("");
            continue;
        }
        if let Some(rest) = tail.strip_prefix("/*") {
            tail = rest.split_once("*/").map(|(_, after)| after).unwrap_or("");
            continue;
        }
        return true;
    }
}
