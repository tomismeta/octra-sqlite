use super::*;

pub(super) fn cmd_setup(args: SetupArgs) -> Result<()> {
    let mut config = load_config()?;
    let interactive = !args.yes && io::stdin().is_terminal();
    if !interactive && !args.yes {
        bail!("setup is interactive; run it in a terminal or pass --yes with flags");
    }

    print_title("Octra SQLite setup");
    let wallet_path = configure_setup_wallet(&args, &mut config, interactive)?;

    let network_default = args
        .network
        .clone()
        .or_else(|| config.network.clone())
        .ok_or_else(|| anyhow!("network is required; pass --network"))?;
    let network = if interactive {
        prompt_network(&network_default)?
    } else {
        network_default
    };

    let rpc = args
        .rpc
        .clone()
        .or_else(|| env::var("OCTRA_RPC_URL").ok())
        .or_else(|| config.rpc_for_network(&network))
        .or_else(|| config.rpc.clone())
        .ok_or_else(|| anyhow!("RPC is required; pass --rpc or set OCTRA_RPC_URL"))?;

    config.network = Some(network.clone());
    config.apply_active_network_profile();
    config.rpc = Some(rpc.clone());
    write_config(&config)?;
    println!();
    print_section("Setup complete");
    print_field("wrote", config_path()?.display().to_string());
    match &wallet_path {
        Some(path) => print_field("wallet", path.display().to_string()),
        None => print_field("wallet", "not configured"),
    }
    print_field("network", &network);
    print_field("rpc", &rpc);
    if let Some(explorer) = config.explorer_for_network(&network) {
        print_field("explorer", explorer);
    }
    println!();
    print_section("Next");
    if wallet_path.is_some() {
        print_command("create a new database", CREATE_DATABASE_COMMAND);
        print_command("browse an existing database", "octra-sqlite open DATABASE");
    } else {
        print_command(
            "browse a public database",
            format!("octra-sqlite open oct://{network}/CIRCLE"),
        );
        print_command(
            "attach a wallet",
            "octra-sqlite wallet attach /path/to/wallet.json",
        );
        print_command("import a wallet", "octra-sqlite wallet import --stdin");
    }
    Ok(())
}

pub(super) fn configure_setup_wallet(
    args: &SetupArgs,
    config: &mut Config,
    interactive: bool,
) -> Result<Option<PathBuf>> {
    if let Some(path) = args.wallet.as_deref() {
        let path = configure_explicit_wallet(config, path)?;
        return Ok(Some(path));
    }
    if let Some(path) = configured_or_discovered_wallet(config)? {
        config.wallet = Some(path.to_string_lossy().to_string());
        return Ok(Some(path));
    }
    if interactive {
        return match prompt_wallet_onboarding(config)? {
            WalletOnboarding::Configured(path) => Ok(Some(path)),
            WalletOnboarding::Walletless => {
                config.wallet = None;
                Ok(None)
            }
        };
    }
    config.wallet = None;
    Ok(None)
}

pub(super) fn cmd_new(args: NewArgs) -> Result<()> {
    let args = resolve_new_args(args)?;
    let name = args
        .name
        .as_deref()
        .ok_or_else(|| anyhow!("database name is required"))?;

    let mut config = load_config()?;
    ensure_new_database_name_available(&args, &config, name)?;
    ensure_wallet_for_database_creation(&args, &mut config)?;
    let init_sql = collect_initializer_sql(&args)?;
    let network = args
        .network
        .clone()
        .or_else(|| config.network.clone())
        .ok_or_else(|| anyhow!("network is required; run octra-sqlite setup or pass --network"))?;
    let control_args = TargetArgs {
        target: None,
        wallet: args.wallet.clone(),
        rpc: args.rpc.clone(),
        caller: args.caller.clone(),
        private_key_b64: args.private_key_b64.clone(),
        public_key_b64: args.public_key_b64.clone(),
    };
    let control_session = build_control_session(&control_args, &network)?;

    let funding_detail = if init_sql.is_empty() {
        format!(
            "requires funded wallet; create budget {} OU",
            args.create_ou
        )
    } else {
        format!(
            "requires funded wallet; create budget {} OU plus initializer writes",
            args.create_ou
        )
    };
    if !args.json {
        println!();
        print_field("funding", funding_detail);
    }
    let read_mode = ReadMode::from(args.read_mode);
    if !args.json {
        if read_mode == ReadMode::Sealed {
            print_field(
                "sealed reads",
                "authenticated wallet access; not encrypted or owner-only",
            );
        }
        print_field(
            "write privacy",
            "SQL and values are visible in Octra transaction history",
        );
    }
    let created = create_circle(&control_session, &args, &network, read_mode)?;
    let target_uri = format!("oct://{}/{}", network, created.circle);
    let mut default_database = false;
    if !args.no_name {
        if let Err(error) = save_new_database_name(&args, &target_uri, &created, &mut config) {
            if !args.json {
                print_circle_recovery(
                    &args,
                    &target_uri,
                    "database name was not saved after Circle creation",
                    false,
                );
            }
            return Err(error.context("database name save failed after Circle creation"));
        }
        default_database = config.default_database.as_deref() == Some(name);
    }

    let mut initializer_results = Vec::new();
    if !init_sql.is_empty() {
        let session_args = TargetArgs {
            target: Some(target_uri.clone()),
            wallet: args.wallet.clone(),
            rpc: Some(control_session.rpc().to_string()),
            caller: args.caller.clone(),
            private_key_b64: args.private_key_b64.clone(),
            public_key_b64: args.public_key_b64.clone(),
        };
        let session = match build_session(&session_args) {
            Ok(session) => session,
            Err(error) => {
                if !args.json {
                    print_circle_recovery(
                        &args,
                        &target_uri,
                        "initializer session failed after Circle creation",
                        !args.no_name,
                    );
                }
                return Err(error.context("initializer session failed after Circle creation"));
            }
        };
        initializer_results = match run_initializer_sql(&session, &args, &init_sql) {
            Ok(results) => results,
            Err(error) => {
                if !args.json {
                    print_circle_recovery(
                        &args,
                        &target_uri,
                        "initializer failed after Circle creation",
                        !args.no_name,
                    );
                }
                return Err(error.context("initializer failed after Circle creation"));
            }
        };
    }

    let readiness = if args.no_wait {
        new_readiness_skipped_json()
    } else {
        let session_args = TargetArgs {
            target: Some(target_uri.clone()),
            wallet: args.wallet.clone(),
            rpc: Some(control_session.rpc().to_string()),
            caller: args.caller.clone(),
            private_key_b64: args.private_key_b64.clone(),
            public_key_b64: args.public_key_b64.clone(),
        };
        match build_session(&session_args) {
            Ok(session) => new_readiness_json(&session),
            Err(error) => json!({
                "checked": false,
                "error": format!("{error:#}"),
            }),
        }
    };
    let manifest_value = new_manifest_json(NewManifestInput {
        args: &args,
        name,
        target_uri: &target_uri,
        network: &network,
        created: &created,
        owner: control_session.caller(),
        rpc: control_session.rpc(),
        init_sql: &init_sql,
        initializer_results: &initializer_results,
        readiness: readiness.clone(),
    });
    let manifest_path = if let Some(path) = &args.manifest {
        write_new_manifest(path, &manifest_value)?;
        Some(path.clone())
    } else {
        None
    };
    if args.json {
        let mut envelope = manifest_value;
        if let Some(object) = envelope.as_object_mut() {
            object.insert("ok".to_string(), Value::Bool(true));
            object.insert("type".to_string(), Value::String("new".to_string()));
            object.insert(
                "schema".to_string(),
                Value::String("octra-sqlite.cli.v1".to_string()),
            );
            if let Some(path) = &manifest_path {
                object.insert(
                    "manifest_path".to_string(),
                    Value::String(path.display().to_string()),
                );
            }
        }
        return print_json(&envelope);
    }

    let followup_target = new_followup_target(name, &target_uri, args.no_name);
    println!();
    print_section("Database ready");
    if args.no_name {
        print_field("created", "(not saved)");
    } else {
        print_field("created", name);
    }
    print_field("uri", database_read_uri(&target_uri, read_mode));
    print_field("read_mode", read_mode.as_str());
    print_field("default", if default_database { "yes" } else { "no" });
    if let Some(path) = manifest_path {
        print_field("manifest", path.display().to_string());
    }
    if let Some(hash) = &created.tx_hash {
        print_field("tx", linked_tx(&network, hash));
    }
    println!();
    print_section("Next");
    print_command("open", format!("octra-sqlite open {followup_target}"));
    print_command(
        "tables",
        format!("octra-sqlite {followup_target} \".tables\""),
    );
    Ok(())
}

pub(super) fn resolve_new_args(mut args: NewArgs) -> Result<NewArgs> {
    if args.name.is_some() {
        return Ok(args);
    }
    if args.json {
        bail!("database name is required with --json");
    }
    if !io::stdin().is_terminal() {
        bail!("database name is required; pass DATABASE or run octra-sqlite new in a terminal");
    }

    let config = load_config()?;
    print_title("Create an Octra SQLite database");
    let name = prompt_required("database name")?;
    if name.trim().is_empty() {
        bail!("database name is required");
    }
    args.name = Some(name.clone());

    if args.network.is_none() {
        let network_default = config
            .network
            .clone()
            .unwrap_or_else(|| "devnet".to_string());
        args.network = Some(prompt_network(&network_default)?);
    }
    args.read_mode = prompt_read_mode(args.read_mode)?;
    if !args.no_name && !args.default {
        args.default = true;
    }
    if args.manifest.is_none() {
        args.manifest = Some(default_new_manifest_path(&name));
    }
    if !prompt_yes_no("create database?", true)? {
        bail!("cancelled");
    }
    Ok(args)
}

pub(super) fn default_new_manifest_path(name: &str) -> PathBuf {
    PathBuf::from(format!("{name}.octra-sqlite.json"))
}

pub(super) fn ensure_wallet_for_database_creation(
    args: &NewArgs,
    config: &mut Config,
) -> Result<()> {
    if args.private_key_b64.is_some() || env::var("OCTRA_PRIVATE_KEY_B64").is_ok() {
        return Ok(());
    }
    if let Some(path) = args.wallet.as_deref() {
        reject_encrypted_oct_wallet(path)?;
        let path = canonical_existing_wallet_path(path)?;
        wallet_file_material(&path)?;
        return Ok(());
    }
    if let Ok(path) = env::var("OCTRA_WALLET") {
        let path = PathBuf::from(path);
        reject_encrypted_oct_wallet(&path)?;
        let path = canonical_existing_wallet_path(&path)?;
        wallet_file_material(&path)?;
        return Ok(());
    }
    if let Some(path) = config.wallet.as_ref().map(PathBuf::from) {
        reject_encrypted_oct_wallet(&path)?;
        if path.is_file() {
            wallet_file_material(&path)?;
            return Ok(());
        }
        print_warning(format!("configured wallet not found at {}", path.display()));
        println!();
    }
    if let Some(path) = discover_wallet_path() {
        reject_encrypted_oct_wallet(&path)?;
        let path = canonical_existing_wallet_path(&path)?;
        wallet_file_material(&path)?;
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        bail!(
            "database creation requires a wallet; run octra-sqlite setup, pass --wallet, or set OCTRA_PRIVATE_KEY_B64"
        );
    }
    match prompt_wallet_onboarding(config)? {
        WalletOnboarding::Configured(_) => Ok(()),
        WalletOnboarding::Walletless => {
            bail!(
                "database creation requires a wallet; walletless mode only works for public-read queries"
            )
        }
    }
}

pub(super) fn database_read_uri(target_uri: &str, _read_mode: ReadMode) -> String {
    target_uri.to_string()
}

pub(super) fn collect_initializer_sql(args: &NewArgs) -> Result<Vec<String>> {
    let mut init_sql = Vec::new();
    if let Some(path) = &args.read {
        init_sql.push(read_sql_file_arg(path)?);
    }
    if let Some(sample) = &args.sample {
        init_sql.push(sample_sql(sample)?);
    }
    if let Some(sql) = &args.sql {
        init_sql.push(sql.clone());
    }
    if !args.sql_args.is_empty() {
        init_sql.extend(args.sql_args.iter().cloned());
    }
    if init_sql.is_empty()
        && let Some(sql) = read_stdin_sql()?
    {
        init_sql.push(sql);
    }
    Ok(init_sql)
}

pub(super) fn run_initializer_sql(
    session: &Session,
    args: &NewArgs,
    init_sql: &[String],
) -> Result<Vec<SqlScriptExecution>> {
    let mut executions = Vec::new();
    for sql in init_sql {
        let mut execution = if args.no_wait {
            submit_sql_script_no_wait(session, sql)?
        } else {
            execute_sql_script_with_progress(session, sql, false, |_| {})?
        };
        for result in &mut execution.results {
            let raw = std::mem::take(result);
            *result = with_explorer(raw, session);
        }
        if !args.json {
            if args.no_wait {
                for result in &execution.results {
                    print_exec_result(result)?;
                }
                print_field(
                    "initializer",
                    format!("{} statements submitted", execution.statements),
                );
            } else {
                print_field(
                    "initializer",
                    format!("{} statements", execution.statements),
                );
            }
        }
        executions.push(execution);
    }
    Ok(executions)
}

pub(super) fn save_new_database_name(
    args: &NewArgs,
    target_uri: &str,
    created: &CreatedCircle,
    config: &mut Config,
) -> Result<()> {
    let name = args
        .name
        .as_deref()
        .ok_or_else(|| anyhow!("database name is required"))?;
    config
        .databases
        .insert(name.to_string(), target_uri.to_string());
    config.database_metadata.insert(
        name.to_string(),
        DatabaseMetadata {
            uri: target_uri.to_string(),
            network: target_uri
                .strip_prefix("oct://")
                .and_then(|value| {
                    value
                        .split_once('/')
                        .map(|(network, _)| network.to_string())
                })
                .unwrap_or_default(),
            circle: created.circle.clone(),
            read_mode: ReadMode::from(args.read_mode),
            privacy_class: deploy_tuple(ReadMode::from(args.read_mode)).0.to_string(),
            browser_mode: deploy_tuple(ReadMode::from(args.read_mode)).1.to_string(),
            resource_mode: deploy_tuple(ReadMode::from(args.read_mode)).2.to_string(),
            owner: created.owner.clone(),
            owner_pubkey: created.auth_patch.owner_pubkey_hex.clone(),
            db_id: created.auth_patch.db_id_hex.clone(),
            code_hash: created.code_hash.clone(),
            code_bytes: created.code_bytes,
            create_tx: created.tx_hash.clone(),
            program_update_tx: None,
        },
    );
    if args.default || config.default_database.is_none() {
        config.default_database = Some(name.to_string());
    }
    write_config(config)?;
    Ok(())
}

pub(super) fn ensure_new_database_name_available(
    args: &NewArgs,
    config: &Config,
    name: &str,
) -> Result<()> {
    if args.no_name {
        return Ok(());
    }
    let Some(existing_uri) = config.databases.get(name) else {
        return Ok(());
    };
    if !args.json {
        print_field("database", name);
        print_field("existing", existing_uri);
        print_field(
            "status",
            format!("octra-sqlite status {}", shell_quote(name)),
        );
        print_field("open", format!("octra-sqlite open {}", shell_quote(name)));
        print_field(
            "remove",
            format!("octra-sqlite database remove {}", shell_quote(name)),
        );
    }
    Err(target_error(format!(
        "database name '{name}' already exists for database URI {existing_uri}"
    )))
}

pub(super) fn print_circle_recovery(args: &NewArgs, target_uri: &str, problem: &str, saved: bool) {
    print_field("recovery", problem);
    print_field(
        "warning",
        "initializer scripts may be partially applied; inspect before retrying",
    );
    print_field("uri", target_uri);
    if saved {
        print_field("saved", "yes");
    } else {
        print_field("saved", "no");
        print_field(
            "recover",
            format!(
                "octra-sqlite database set {} {}",
                shell_quote(args.name.as_deref().unwrap_or("database")),
                shell_quote(target_uri)
            ),
        );
    }
    let followup_target = if saved {
        args.name.as_deref().unwrap_or(target_uri)
    } else {
        target_uri
    };
    print_field(
        "inspect",
        format!("octra-sqlite {} \".tables\"", shell_quote(followup_target)),
    );
    print_field(
        "open",
        format!("octra-sqlite open {}", shell_quote(followup_target)),
    );
    if let Some(path) = &args.read {
        let dot_command = format!(".read {}", dot_arg_quote(&path.to_string_lossy()));
        print_field(
            "retry",
            format!(
                "octra-sqlite {} {}",
                shell_quote(followup_target),
                shell_quote(&dot_command)
            ),
        );
    } else {
        print_field(
            "retry",
            "inspect first, then rerun the initializer SQL against the saved database",
        );
    }
}

pub(super) struct NewManifestInput<'a> {
    pub(super) args: &'a NewArgs,
    pub(super) name: &'a str,
    pub(super) target_uri: &'a str,
    pub(super) network: &'a str,
    pub(super) created: &'a CreatedCircle,
    pub(super) owner: &'a str,
    pub(super) rpc: &'a str,
    pub(super) init_sql: &'a [String],
    pub(super) initializer_results: &'a [SqlScriptExecution],
    pub(super) readiness: Value,
}

pub(super) fn new_manifest_json(input: NewManifestInput<'_>) -> Value {
    let args = input.args;
    let initializer_plans = input
        .init_sql
        .iter()
        .filter_map(|sql| {
            plan_sql_script(sql)
                .ok()
                .map(|plan| script_plan_json(&plan))
        })
        .collect::<Vec<_>>();
    let initializer_writes = input
        .initializer_results
        .iter()
        .flat_map(|execution| execution.results.iter().map(write_result_summary))
        .collect::<Vec<_>>();
    let initializer_sql = if input.init_sql.is_empty() {
        None
    } else {
        Some(input.init_sql.join("\n"))
    };
    let initializer_sha256 = initializer_sql
        .as_deref()
        .map(|sql| sha256_hex(sql.as_bytes()));
    let read_mode = ReadMode::from(args.read_mode);
    let (privacy_class, browser_mode, resource_mode) = deploy_tuple(read_mode);
    json!({
        "manifest_version": "octra-sqlite.database.v1",
        "database": {
            "name": if args.no_name { Value::Null } else { Value::String(input.name.to_string()) },
            "uri": input.target_uri,
            "read_uri": database_read_uri(input.target_uri, read_mode),
            "network": input.network,
            "circle": input.created.circle.clone(),
            "circle_url": explorer_circle_url(input.network, &input.created.circle),
            "rpc": input.rpc,
            "read": {
                "mode": read_mode.as_str(),
                "privacy_class": privacy_class,
                "browser_mode": browser_mode,
                "resource_mode": resource_mode,
            },
        },
        "owner": {
            "wallet": input.owner,
            "write_auth": "OSW1 owner write intent",
            "owner_pubkey": input.created.auth_patch.owner_pubkey_hex.clone(),
            "db_id": input.created.auth_patch.db_id_hex.clone(),
        },
        "confidentiality": {
            "encrypted": false,
            "read_access": if read_mode == ReadMode::Sealed { "authenticated_wallet" } else { "public" },
            "read_owner_only": false,
            "write_sql_visible_in_transaction_history": true,
        },
        "program": {
            "runtime": "wasm_v1",
            "wasm_hash": input.created.code_hash.clone(),
            "wasm_bytes": input.created.code_bytes,
            "source": "bundled",
        },
        "create": {
            "tx_hash": input.created.tx_hash.clone(),
            "tx_url": input.created.tx_hash.as_deref().and_then(|hash| explorer_tx_url(input.network, hash)),
            "confirmation": input.created.confirmation.clone(),
        },
        "initializer": {
            "present": !input.init_sql.is_empty(),
            "schema_file": args.read.as_ref().map(|path| path.display().to_string()),
            "source_count": input.init_sql.len(),
            "source_bytes": input.init_sql.iter().map(|sql| sql.len()).sum::<usize>(),
            "sha256": initializer_sha256,
            "plans": initializer_plans,
            "statements": input.initializer_results.iter().map(|execution| execution.statements).sum::<usize>(),
            "batches": input.initializer_results.iter().map(|execution| execution.batches).sum::<usize>(),
            "writes": initializer_writes,
        },
        "readiness": input.readiness,
        "next": {
            "status": format!("octra-sqlite status {}", if args.no_name { input.target_uri } else { input.name }),
            "tables": format!("octra-sqlite {} \".tables\"", if args.no_name { input.target_uri } else { input.name }),
            "open": format!("octra-sqlite open {}", if args.no_name { input.target_uri } else { input.name }),
        }
    })
}

pub(super) fn write_new_manifest(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, format_json(value)? + "\n")
        .with_context(|| format!("writing {}", path.display()))
}

pub(super) fn new_readiness_skipped_json() -> Value {
    json!({
        "checked": false,
        "reason": "no_wait",
    })
}

pub(super) fn new_readiness_json(session: &Session) -> Value {
    let mut readiness = Map::new();
    let mut errors = Map::new();

    match program_info(session) {
        Ok(_) => {
            readiness.insert("circle_reachable".to_string(), Value::Bool(true));
        }
        Err(error) => {
            readiness.insert("circle_reachable".to_string(), Value::Bool(false));
            errors.insert(
                "circle_reachable".to_string(),
                Value::String(format!("{error:#}")),
            );
        }
    }
    match auth_info(session) {
        Ok(auth) => {
            readiness.insert("auth_readable".to_string(), Value::Bool(true));
            readiness.insert(
                "owner_write_configured".to_string(),
                Value::Bool(auth.configured),
            );
        }
        Err(error) => {
            readiness.insert("auth_readable".to_string(), Value::Bool(false));
            readiness.insert("owner_write_configured".to_string(), Value::Bool(false));
            errors.insert(
                "auth_readable".to_string(),
                Value::String(format!("{error:#}")),
            );
        }
    }
    match view(session, "storage_info", vec![]) {
        Ok(storage) => {
            let initialized = storage
                .get("page_count")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 0;
            readiness.insert("storage_initialized".to_string(), Value::Bool(initialized));
        }
        Err(error) => {
            readiness.insert("storage_initialized".to_string(), Value::Bool(false));
            errors.insert(
                "storage_initialized".to_string(),
                Value::String(format!("{error:#}")),
            );
        }
    }
    match query_typed(session, "select sqlite_version() as sqlite_version;") {
        Ok(result) => {
            readiness.insert("sqlite_ready".to_string(), Value::Bool(true));
            readiness.insert("query_ready".to_string(), Value::Bool(true));
            readiness.insert(
                "sqlite_version".to_string(),
                first_result_cell(&result)
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
        }
        Err(error) => {
            readiness.insert("sqlite_ready".to_string(), Value::Bool(false));
            readiness.insert("query_ready".to_string(), Value::Bool(false));
            errors.insert(
                "sqlite_ready".to_string(),
                Value::String(format!("{error:#}")),
            );
        }
    }
    let ready = [
        "circle_reachable",
        "auth_readable",
        "sqlite_ready",
        "query_ready",
    ]
    .into_iter()
    .all(|key| readiness.get(key).and_then(Value::as_bool) == Some(true));

    json!({
        "checked": true,
        "ready": ready,
        "items": readiness,
        "errors": errors,
    })
}

pub(super) fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '_' | '-' | '.' | '/' | ':' | '@' | '=' | ',')
        })
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

pub(super) fn dot_arg_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':'))
    {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('"', "\"\""))
    }
}

pub(super) fn new_followup_target<'a>(
    name: &'a str,
    target_uri: &'a str,
    no_name: bool,
) -> &'a str {
    if no_name { target_uri } else { name }
}

pub(super) fn cmd_wallet(command: WalletCommand) -> Result<()> {
    let result = match command {
        WalletCommand::Status(args) => cmd_wallet_status(args),
        WalletCommand::Attach(args) => cmd_wallet_attach(args),
        WalletCommand::Import(args) => cmd_wallet_import(args),
    };
    result.map_err(|error| with_fallback_code(error, "wallet_error"))
}

pub(super) enum WalletOnboarding {
    Configured(PathBuf),
    Walletless,
}

pub(super) fn configured_or_discovered_wallet(config: &Config) -> Result<Option<PathBuf>> {
    if let Ok(path) = env::var("OCTRA_WALLET") {
        let path = PathBuf::from(path);
        reject_encrypted_oct_wallet(&path)?;
        let path = canonical_existing_wallet_path(&path)?;
        wallet_file_material(&path)?;
        return Ok(Some(path));
    }
    if let Some(path) = config.wallet.as_ref().map(PathBuf::from) {
        reject_encrypted_oct_wallet(&path)?;
        if path.is_file() {
            let path = canonical_existing_wallet_path(&path)?;
            wallet_file_material(&path)?;
            return Ok(Some(path));
        }
        print_warning(format!("configured wallet not found at {}", path.display()));
        println!();
    }
    if let Some(path) = discover_wallet_path() {
        reject_encrypted_oct_wallet(&path)?;
        let path = canonical_existing_wallet_path(&path)?;
        wallet_file_material(&path)?;
        return Ok(Some(path));
    }
    Ok(None)
}

pub(super) fn configure_explicit_wallet(config: &mut Config, path: &Path) -> Result<PathBuf> {
    reject_encrypted_oct_wallet(path)?;
    let path = canonical_existing_wallet_path(path)?;
    let material = wallet_file_material(&path)?;
    config.wallet = Some(path.to_string_lossy().to_string());
    print_field("wallet", path.display().to_string());
    print_field("address", &material.address);
    Ok(path)
}

pub(super) fn prompt_wallet_onboarding(config: &mut Config) -> Result<WalletOnboarding> {
    print_section("Wallet");
    print_field("status", "not configured");
    println!("{}", dim("Choose a wallet source."));
    print_choice(1, "Import wallet.json from Octra wallet generator");
    print_choice(2, "Attach existing wallet.json");
    print_choice(3, "Paste private key securely");
    print_choice(4, "Skip wallet for now (public-read only)");
    print_choice(5, "Cancel");
    println!();
    loop {
        let choice = prompt_default("wallet", "1")?;
        match choice.trim() {
            "1" => return import_wallet_from_generator(config).map(WalletOnboarding::Configured),
            "2" => return attach_wallet_interactive(config).map(WalletOnboarding::Configured),
            "3" => return paste_wallet_interactive(config).map(WalletOnboarding::Configured),
            "4" => {
                print_field(
                    "wallet",
                    "skipped; sealed reads and writes require a wallet",
                );
                println!();
                return Ok(WalletOnboarding::Walletless);
            }
            "5" => bail!("wallet setup cancelled"),
            _ => println!("Enter 1, 2, 3, 4, or 5."),
        }
    }
}

pub(super) fn import_wallet_from_generator(config: &mut Config) -> Result<PathBuf> {
    println!();
    print_section("Import generated wallet");
    print_field("url", OFFICIAL_WALLET_GENERATOR_URL);
    print_field(
        "step",
        "generate a wallet with the official Octra generator, save wallet.json, then paste its local path here.",
    );
    print_warning(
        "only use the official Octra generator; private-key generator URLs are phishing targets.",
    );
    let source = prompt_path_no_default("wallet JSON path")?;
    reject_encrypted_oct_wallet(&source)?;
    let source = canonical_existing_wallet_path(&source)?;
    let material = wallet_file_material(&source)?;
    let output = default_wallet_output_path()?;
    let output = absolute_wallet_output_path(&output)?;
    if source != output {
        copy_wallet_json(&source, &output, false)?;
        config.wallet = Some(output.to_string_lossy().to_string());
        write_config(config)?;
        print_field("wallet", output.display().to_string());
        print_field("address", &material.address);
        print_field(
            "next",
            format!("delete the downloaded copy at {}", source.display()),
        );
        warn_wallet_permissions_if_needed(&output);
        return Ok(output);
    }
    config.wallet = Some(source.to_string_lossy().to_string());
    restrict_wallet_permissions_if_possible(&source)?;
    write_config(config)?;
    print_field("wallet", source.display().to_string());
    print_field("address", &material.address);
    Ok(source)
}

pub(super) fn attach_wallet_interactive(config: &mut Config) -> Result<PathBuf> {
    println!();
    print_section("Attach wallet");
    print_field(
        "expects",
        "wallet.json with address and private key material",
    );
    let path = prompt_path_no_default("wallet JSON path")?;
    let path = configure_explicit_wallet(config, &path)?;
    write_config(config)?;
    Ok(path)
}

pub(super) fn paste_wallet_interactive(config: &mut Config) -> Result<PathBuf> {
    println!();
    print_section("Paste private key");
    print_field("used for", "creating databases and signing writes");
    print_field(
        "saved at",
        "~/.octra/wallet.json with restricted permissions where supported",
    );
    print_field(
        "secret",
        "input is masked with * characters and not stored in shell history",
    );
    let private_key = Zeroizing::new(read_tty_secret("private key")?);
    let material = wallet_material_from_private_key(private_key.as_str(), None)?;
    let output = absolute_wallet_output_path(&default_wallet_output_path()?)?;
    write_wallet_json(&output, &material, false)?;
    config.wallet = Some(output.to_string_lossy().to_string());
    write_config(config)?;
    print_field("wallet", output.display().to_string());
    print_field("address", &material.address);
    warn_wallet_permissions_if_needed(&output);
    Ok(output)
}

pub(super) fn cmd_wallet_attach(args: WalletAttachArgs) -> Result<()> {
    reject_encrypted_oct_wallet(&args.path)?;
    let path = canonical_existing_wallet_path(&args.path)?;
    let material = wallet_file_material(&path)?;
    let mut config = load_config()?;
    config.wallet = Some(path.to_string_lossy().to_string());
    write_config(&config)?;
    let config_path = config_path()?;
    if args.json {
        print_json(&json!({
            "ok": true,
            "type": "wallet_attach",
            "schema": "octra-sqlite.cli.v1",
            "wallet": {
                "path": path.display().to_string(),
                "address": material.address,
            },
            "config": {
                "path": config_path.display().to_string(),
                "active_wallet": path.display().to_string(),
            },
        }))?;
        return Ok(());
    }
    print_field("wallet", path.display().to_string());
    print_field("address", &material.address);
    print_field("wrote", config_path.display().to_string());
    print_field("next", "octra-sqlite wallet status");
    Ok(())
}

pub(super) fn cmd_wallet_import(args: WalletImportArgs) -> Result<()> {
    let mut config = load_config()?;
    let output = args
        .output
        .clone()
        .or_else(|| config.wallet.as_ref().map(PathBuf::from))
        .unwrap_or(default_wallet_output_path()?);
    let output = absolute_wallet_output_path(&output)?;
    let material = if args.stdin {
        if args.source.is_some() {
            bail!("wallet import accepts either PATH or --stdin, not both");
        }
        let private_key = Zeroizing::new(read_stdin_secret(
            "wallet import --stdin requires a private key on stdin",
        )?);
        wallet_material_from_private_key(private_key.as_str(), None)?
    } else if let Some(source) = args.source.as_deref() {
        reject_encrypted_oct_wallet(source)?;
        let source = canonical_existing_wallet_path(source)?;
        wallet_file_material(&source)?
    } else {
        bail!("wallet import requires a plaintext wallet PATH or --stdin");
    };
    write_wallet_json(&output, &material, args.force)?;
    if !args.no_use {
        config.wallet = Some(output.to_string_lossy().to_string());
        write_config(&config)?;
    }
    let config_path = config_path()?;
    if args.json {
        print_json(&json!({
            "ok": true,
            "type": "wallet_import",
            "schema": "octra-sqlite.cli.v1",
            "wallet": {
                "path": output.display().to_string(),
                "address": material.address,
            },
            "config": {
                "path": config_path.display().to_string(),
                "active_wallet": if args.no_use { Value::Null } else { Value::String(output.display().to_string()) },
            },
        }))?;
        return Ok(());
    }
    print_field("wallet", output.display().to_string());
    print_field("address", &material.address);
    if args.no_use {
        print_field("config", "unchanged");
    } else {
        print_field("wrote", config_path.display().to_string());
    }
    warn_wallet_permissions_if_needed(&output);
    print_field("next", "octra-sqlite wallet status");
    Ok(())
}

pub(super) fn cmd_wallet_status(args: WalletStatusArgs) -> Result<()> {
    let mut report = StatusReport::new("wallet_status", args.json);
    let config = load_config()?;
    let wallet_path = resolve_wallet_path(&args.target, &config);
    match wallet_path.as_deref() {
        Some(path) => {
            if path.exists() {
                report.ok("wallet", path.display().to_string());
                report_wallet_permissions(&mut report, path);
            } else {
                report.fail("wallet", format!("not found at {}", path.display()));
            }
        }
        None => report.warn(
            "wallet",
            "not configured; pass --wallet or set wallet in config",
        ),
    }
    match wallet_caller(wallet_path.as_deref(), args.target.caller.as_deref()) {
        Ok(Some(caller)) => report.ok("caller", caller),
        Ok(None) => report.warn("caller", "not found in wallet/env"),
        Err(error) => report.fail("caller", error.to_string()),
    }
    match build_session(&args.target) {
        Ok(session) => {
            report.ok("network", &session.target().network);
            report.ok("rpc", session.rpc());
            report.ok("database", canonical_database_uri(session.target()));
            match program_info(&session) {
                Ok(info) => {
                    if let Some(owner) = program_owner(&info) {
                        report.ok("circle owner", owner);
                        if owner == session.caller() {
                            report.ok("circle owner wallet", "current wallet");
                        } else {
                            report.warn(
                                "circle owner wallet",
                                "current wallet is not the Circle owner",
                            );
                        }
                    }
                }
                Err(error) => report.warn("circle owner", format!("unavailable: {error:#}")),
            }
            match auth_info(&session) {
                Ok(auth) if auth.configured => match auth.owner_pubkey.as_deref() {
                    Some(owner_pubkey) => match session.intent_public_key() {
                        Ok(wallet_pubkey) if hex::encode(wallet_pubkey) == owner_pubkey => {
                            report.ok("write wallet", "current wallet can write")
                        }
                        Ok(_) => report.warn("write wallet", "current wallet is read-only"),
                        Err(error) => report.warn(
                            "write wallet",
                            format!("could not derive wallet public key: {error:#}"),
                        ),
                    },
                    None => report.warn("write wallet", "auth_info missing owner public key"),
                },
                Ok(_) => report.warn("write wallet", "database is not owner-personalized"),
                Err(error) => {
                    report.warn("write wallet", format!("auth_info unavailable: {error:#}"))
                }
            }
        }
        Err(error) => report.warn(
            "target",
            format!("skipped target checks; could not build session: {error:#}"),
        ),
    }
    report.finish("wallet")
}

pub(super) fn reject_encrypted_oct_wallet(path: &Path) -> Result<()> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("oct"))
    {
        return Err(wallet_error(
            "webcli .oct wallets are encrypted and need PIN-based decryption; export/import the private key with `octra-sqlite wallet import --stdin` or attach a plaintext wallet JSON",
        ));
    }
    Ok(())
}

pub(super) fn canonical_existing_wallet_path(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path)
        .map_err(|error| wallet_error(format!("wallet not found at {}: {error}", path.display())))
}

pub(super) fn default_wallet_output_path() -> Result<PathBuf> {
    Ok(config_path()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wallet.json"))
}

pub(super) fn absolute_wallet_output_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(env::current_dir()?.join(path))
}

pub(super) fn read_stdin_secret(error: &str) -> Result<String> {
    if io::stdin().is_terminal() {
        bail!("{error}");
    }
    let mut secret = String::new();
    io::stdin().read_to_string(&mut secret)?;
    Ok(secret)
}

pub(super) fn read_tty_secret(prompt: &str) -> Result<String> {
    if !io::stdin().is_terminal() {
        bail!(
            "interactive private-key import requires a terminal; use wallet import --stdin for automation"
        );
    }
    let prompt = format!("{}: ", dim(prompt));
    let config = rpassword::ConfigBuilder::new()
        .password_feedback_mask('*')
        .build();
    rpassword::prompt_password_with_config(prompt, config)
        .context("reading private key from terminal")
}

pub(super) fn write_wallet_json(path: &Path, material: &WalletMaterial, force: bool) -> Result<()> {
    #[derive(serde::Serialize)]
    struct WalletJson<'a> {
        address: &'a str,
        private_key_b64: &'a str,
        public_key_b64: &'a str,
    }

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let payload = WalletJson {
        address: &material.address,
        private_key_b64: &material.private_key_b64,
        public_key_b64: &material.public_key_b64,
    };
    let mut text = serde_json::to_string_pretty(&payload)? + "\n";
    let result = if force {
        crate::private_file::atomic_replace(path, text.as_bytes())
    } else {
        crate::private_file::write_new(path, text.as_bytes())
    };
    text.zeroize();
    result.with_context(|| format!("writing wallet {}", path.display()))?;
    Ok(())
}

pub(super) fn copy_wallet_json(source: &Path, output: &Path, force: bool) -> Result<()> {
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut bytes =
        fs::read(source).with_context(|| format!("reading wallet {}", source.display()))?;
    let result = if force {
        crate::private_file::atomic_replace(output, &bytes)
    } else {
        crate::private_file::write_new(output, &bytes)
    };
    bytes.zeroize();
    result.with_context(|| format!("writing wallet {}", output.display()))?;
    Ok(())
}

pub(super) fn warn_wallet_permissions_if_needed(path: &Path) {
    let _ = path;
    #[cfg(not(unix))]
    println!(
        "{} could not automatically restrict wallet file permissions on this OS; protect {}",
        strong("warning:"),
        path.display()
    );
}

pub(super) fn restrict_wallet_permissions_if_possible(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting wallet permissions for {}", path.display()))?;
    #[cfg(not(unix))]
    warn_wallet_permissions_if_needed(path);
    Ok(())
}
