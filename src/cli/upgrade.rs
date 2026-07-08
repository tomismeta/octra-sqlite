use super::*;

pub(super) const UPGRADE_WRITE_SMOKE_SQL: &str =
    "create table if not exists octra_sqlite_upgrade_smoke(id integer primary key, checked_at text not null);
insert into octra_sqlite_upgrade_smoke(checked_at) values (datetime('now'));
drop table octra_sqlite_upgrade_smoke;";

struct UpgradeSnapshot {
    program_info: Value,
    storage: Value,
    auth: AuthInfo,
    sqlite_version: String,
    code_hash: String,
    code_bytes: Option<u64>,
    storage_generation: Option<u64>,
    owner_sequence: Option<u64>,
}

struct PreparedUpgradeWasm {
    source: String,
    bytes: Vec<u8>,
    hash: String,
    patch: AuthPatch,
}

struct RecoveredWasm {
    bytes: Vec<u8>,
    hash: String,
    source: String,
    tx_hash: Option<String>,
}

struct ProgramUpdateOutcome {
    submitted: Value,
    tx_hash: Option<String>,
    confirmation: Option<Value>,
    program_info: Value,
}

struct UpgradeBundlePaths {
    bundle_dir: PathBuf,
    backup: PathBuf,
    previous_wasm: PathBuf,
    manifest: PathBuf,
}

pub(super) fn cmd_upgrade(args: UpgradeArgs) -> Result<()> {
    if args.target.as_deref() == Some("rollback") {
        return cmd_upgrade_rollback(args);
    }
    if args.rollback_bundle.is_some() {
        bail!("upgrade rollback BUNDLE is the only upgrade sub-flow that accepts a bundle path");
    }
    cmd_upgrade_apply(args)
}

fn cmd_upgrade_apply(mut args: UpgradeArgs) -> Result<()> {
    let requested = resolve_upgrade_database_arg(&mut args)?;
    let target_args = upgrade_target_args(&args, Some(requested.clone()));
    let session = build_session(&target_args)?;
    let before = upgrade_snapshot(&session).context("reading current database state")?;
    ensure_upgrade_owner(&session, &before)?;
    let target_wasm = prepare_upgrade_wasm(&before.auth)?;

    let already_current = before.code_hash == target_wasm.hash;
    let rollback = if already_current {
        None
    } else {
        recover_live_wasm(&session, &before, args.previous_wasm.as_deref())?
    };
    if !already_current && rollback.is_none() && !args.allow_no_rollback {
        bail!(
            "could not recover the currently deployed WASM for rollback; pass --previous-wasm PATH, set OCTRA_SQLITE_PREVIOUS_WASM, or pass --allow-no-rollback to continue without rollback bytes"
        );
    }

    let dry_run = args.dry_run;
    let bundle_paths = if already_current {
        None
    } else {
        Some(default_or_requested_upgrade_bundle_paths(
            args.backup_dir.as_deref(),
            &session,
            &before,
        )?)
    };
    let mut previous_wasm_path = None;

    let plan = upgrade_plan_json(
        &session,
        &before,
        &target_wasm,
        rollback.as_ref(),
        bundle_paths.as_ref(),
        dry_run,
        already_current,
    );
    if dry_run {
        if args.json {
            return print_json(&plan);
        }
        print_upgrade_plan(&plan);
        return Ok(());
    }

    if already_current {
        if args.json {
            return print_json(&plan);
        }
        print_field("database", canonical_database_uri(session.target()));
        print_field("status", "already running bundled SQLite engine");
        print_field("sqlite", &before.sqlite_version);
        print_field("program hash", &before.code_hash);
        return Ok(());
    }

    if !args.yes {
        if !io::stdin().is_terminal() {
            bail!("upgrade requires --yes when stdin is not a terminal");
        }
        if args.json {
            bail!("upgrade --json writes require --yes; use --dry-run --json for preflight");
        }
        prompt_upgrade_wizard(&mut args, &plan, bundle_paths.as_ref())?;
    }

    let mut backup_json = json!({
        "skipped": args.skip_backup,
        "reason": if args.skip_backup {
            "operator_requested"
        } else {
            ""
        },
    });

    let bundle_paths =
        bundle_paths.ok_or_else(|| anyhow!("upgrade bundle path was not prepared"))?;
    create_private_dir(&bundle_paths.bundle_dir)?;
    if let Some(recovered) = rollback.as_ref() {
        let path = bundle_paths.previous_wasm.clone();
        write_private_bytes(&path, &recovered.bytes, true)?;
        previous_wasm_path = Some(path);
    }

    if !args.skip_backup {
        let backup = take_upgrade_backup(&session, &bundle_paths.backup, args.require_integrity)?;
        backup_json = backup;
    }

    let guard = upgrade_snapshot(&session).context("reading database state after backup")?;
    ensure_upgrade_guard_unchanged(&before, &guard, "pre-upgrade")?;

    let update = submit_program_update(&session, &target_wasm.bytes, &args.ou, &target_wasm.hash)?;
    let after = upgrade_snapshot(&session).context("reading database state after upgrade")?;
    ensure_sqlite_version(&after, SQLITE_VERSION)?;
    let clean_rollback = clean_rollback_state(&guard, &after);

    let write_smoke = if args.write_smoke {
        let result = with_explorer(
            exec_sql(&session, UPGRADE_WRITE_SMOKE_SQL, false)?,
            &session,
        );
        Some(write_envelope(&session, result, Some(3)))
    } else {
        None
    };

    let saved_metadata = save_database_metadata(
        &session,
        &target_wasm.patch.owner_pubkey_hex,
        &target_wasm.patch.db_id_hex,
        &target_wasm.hash,
        target_wasm.bytes.len(),
        update.tx_hash.clone(),
    )?;

    let manifest = upgrade_manifest_json(UpgradeManifestInput {
        session: &session,
        before: &before,
        after: &after,
        target_wasm: &target_wasm,
        rollback: rollback.as_ref(),
        previous_wasm_path: previous_wasm_path.as_deref(),
        backup: &backup_json,
        update: &update,
        clean_rollback,
        write_smoke: write_smoke.as_ref(),
        saved_metadata: &saved_metadata,
    });
    let manifest_file = bundle_paths.manifest.clone();
    write_private_json(&manifest_file, &manifest, true)?;

    let mut envelope = upgrade_result_json(UpgradeResultInput {
        session: &session,
        before: &before,
        after: &after,
        target_wasm: &target_wasm,
        rollback: rollback.as_ref(),
        backup: &backup_json,
        update: &update,
        manifest: &manifest,
        clean_rollback,
        previous_wasm_path: previous_wasm_path.as_deref(),
        write_smoke,
        saved_metadata,
    });
    if let Some(object) = envelope.as_object_mut() {
        object.insert(
            "bundle".to_string(),
            bundle_json(&bundle_paths.bundle_dir, Some(&manifest_file)),
        );
    }
    if args.json {
        print_json(&envelope)?;
    } else {
        print_field("database", canonical_database_uri(session.target()));
        print_field(
            "sqlite",
            format!("{} -> {}", before.sqlite_version, SQLITE_VERSION),
        );
        print_field(
            "program hash",
            format!("{} -> {}", before.code_hash, target_wasm.hash),
        );
        if let Some(hash) = update.tx_hash.as_deref() {
            print_field("tx", linked_tx(&session.target().network, hash));
        }
        print_field(
            "backup",
            backup_json
                .get("path")
                .map(value_to_string)
                .unwrap_or_else(|| "skipped".to_string()),
        );
        print_field("bundle", bundle_paths.bundle_dir.display().to_string());
        print_field("rollback clean", rollback_clean_label(clean_rollback));
        print_field(
            "verify",
            format!("octra-sqlite status {} --ready", requested),
        );
    }
    Ok(())
}

struct UpgradeManifestInput<'a> {
    session: &'a Session,
    before: &'a UpgradeSnapshot,
    after: &'a UpgradeSnapshot,
    target_wasm: &'a PreparedUpgradeWasm,
    rollback: Option<&'a RecoveredWasm>,
    previous_wasm_path: Option<&'a Path>,
    backup: &'a Value,
    update: &'a ProgramUpdateOutcome,
    clean_rollback: Option<bool>,
    write_smoke: Option<&'a Value>,
    saved_metadata: &'a [String],
}

struct UpgradeResultInput<'a> {
    session: &'a Session,
    before: &'a UpgradeSnapshot,
    after: &'a UpgradeSnapshot,
    target_wasm: &'a PreparedUpgradeWasm,
    rollback: Option<&'a RecoveredWasm>,
    backup: &'a Value,
    update: &'a ProgramUpdateOutcome,
    manifest: &'a Value,
    clean_rollback: Option<bool>,
    previous_wasm_path: Option<&'a Path>,
    write_smoke: Option<Value>,
    saved_metadata: Vec<String>,
}

fn cmd_upgrade_rollback(args: UpgradeArgs) -> Result<()> {
    let bundle = args
        .rollback_bundle
        .as_deref()
        .ok_or_else(|| anyhow!("upgrade rollback requires a bundle directory"))?;
    let manifest_path = bundle.join("upgrade.json");
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_str(&manifest_text)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;
    if manifest.get("schema").and_then(Value::as_str) != Some(UPGRADE_BUNDLE_SCHEMA) {
        bail!(
            "{} is not an octra-sqlite upgrade bundle",
            manifest_path.display()
        );
    }
    let uri = manifest
        .pointer("/database/uri")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("upgrade bundle missing database.uri"))?;
    let target_args = upgrade_target_args(&args, Some(uri.to_string()));
    let session = build_session(&target_args)?;
    let before = upgrade_snapshot(&session).context("reading current database state")?;
    ensure_upgrade_owner(&session, &before)?;

    let upgraded_hash = manifest
        .pointer("/to/code_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("upgrade bundle missing to.code_hash"))?;
    if before.code_hash != upgraded_hash {
        bail!(
            "live program hash {} does not match bundle upgraded hash {upgraded_hash}; refusing rollback",
            before.code_hash
        );
    }

    let previous_wasm_rel = manifest
        .pointer("/rollback/wasm")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("upgrade bundle does not contain rollback WASM"))?;
    let previous_wasm_path = if Path::new(previous_wasm_rel).is_absolute() {
        PathBuf::from(previous_wasm_rel)
    } else {
        bundle.join(previous_wasm_rel)
    };
    let previous_wasm = fs::read(&previous_wasm_path)
        .with_context(|| format!("reading {}", previous_wasm_path.display()))?;
    let previous_hash = sha256_hex(&previous_wasm);
    let expected_previous_hash = manifest
        .pointer("/from/code_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("upgrade bundle missing from.code_hash"))?;
    if previous_hash != expected_previous_hash {
        bail!(
            "rollback WASM hash {previous_hash} does not match bundle from.code_hash {expected_previous_hash}"
        );
    }

    let guard_sequence = manifest
        .pointer("/rollback/guard/owner_sequence")
        .and_then(Value::as_u64);
    let guard_generation = manifest
        .pointer("/rollback/guard/storage_generation")
        .and_then(Value::as_u64);
    let writes_after_upgrade = !optional_counter_unchanged(before.owner_sequence, guard_sequence)
        .unwrap_or(false)
        || !optional_counter_unchanged(before.storage_generation, guard_generation)
            .unwrap_or(false);
    if writes_after_upgrade && !args.force_after_writes {
        bail!(
            "database changed after upgrade; rollback would cross writes. Re-run with --force-after-writes after review."
        );
    }

    let mut forced_backup = Value::Null;
    if writes_after_upgrade && !args.dry_run {
        let path = bundle.join(format!("rollback-force-backup-{}.sqlite", unix_seconds()));
        forced_backup = take_upgrade_backup(&session, &path, args.require_integrity)?;
    }

    let plan = json!({
        "ok": true,
        "type": "upgrade_rollback",
        "schema": "octra-sqlite.cli.v1",
        "mode": if args.dry_run { "dry_run" } else { "planned" },
        "database": database_identity(&session),
        "from": snapshot_program_json(&before),
        "to": manifest.get("from").cloned().unwrap_or(Value::Null),
        "bundle": bundle_json(bundle, Some(&manifest_path)),
        "writes_after_upgrade": writes_after_upgrade,
        "force_after_writes": args.force_after_writes,
    });
    if args.dry_run {
        if args.json {
            return print_json(&plan);
        }
        print_upgrade_plan(&plan);
        return Ok(());
    }

    if !args.yes {
        if !io::stdin().is_terminal() {
            bail!("upgrade rollback requires --yes when stdin is not a terminal");
        }
        print_upgrade_plan(&plan);
        println!();
        if !prompt_yes_no("rollback this database", false)? {
            bail!("rollback cancelled");
        }
    }

    let update = submit_program_update(&session, &previous_wasm, &args.ou, &previous_hash)?;
    let after = upgrade_snapshot(&session).context("reading database state after rollback")?;
    save_database_metadata(
        &session,
        before
            .auth
            .owner_pubkey
            .as_deref()
            .ok_or_else(|| anyhow!("auth_info missing owner_pubkey"))?,
        &before.auth.db_id,
        &previous_hash,
        previous_wasm.len(),
        update.tx_hash.clone(),
    )?;

    let envelope = json!({
        "ok": true,
        "type": "upgrade_rollback",
        "schema": "octra-sqlite.cli.v1",
        "mode": "applied",
        "database": database_identity(&session),
        "from": snapshot_program_json(&before),
        "to": snapshot_program_json(&after),
        "bundle": bundle_json(bundle, Some(&manifest_path)),
        "writes_after_upgrade": writes_after_upgrade,
        "forced_backup": forced_backup,
        "transaction": program_update_json(&session, &update),
    });
    if args.json {
        print_json(&envelope)?;
    } else {
        print_field("database", canonical_database_uri(session.target()));
        print_field(
            "program hash",
            format!("{} -> {}", before.code_hash, previous_hash),
        );
        if let Some(hash) = update.tx_hash.as_deref() {
            print_field("tx", linked_tx(&session.target().network, hash));
        }
        if writes_after_upgrade {
            print_field(
                "forced backup",
                envelope
                    .pointer("/forced_backup/path")
                    .map(value_to_string)
                    .unwrap_or_else(|| "written".to_string()),
            );
        }
    }
    Ok(())
}

fn upgrade_target_args(args: &UpgradeArgs, target: Option<String>) -> TargetArgs {
    TargetArgs {
        target,
        wallet: args.wallet.clone(),
        rpc: args.rpc.clone(),
        caller: args.caller.clone(),
        private_key_b64: args.private_key_b64.clone(),
        public_key_b64: args.public_key_b64.clone(),
    }
}

fn resolve_upgrade_database_arg(args: &mut UpgradeArgs) -> Result<String> {
    if let Some(target) = args.target.clone() {
        return Ok(target);
    }
    if args.yes || args.json || !io::stdin().is_terminal() {
        bail!("upgrade requires DATABASE for non-interactive use");
    }
    let config = load_config().unwrap_or_default();
    print_title("Upgrade");
    print_section("Database");
    if let Some(default) = config.default_database.as_deref() {
        print_field("default", default);
        let target = prompt_default("database", default)?;
        args.target = Some(target.clone());
        println!();
        return Ok(target);
    }
    if !config.databases.is_empty() {
        println!("{}", dim("saved databases:"));
        for name in config.databases.keys() {
            println!("  {name}");
        }
    }
    let target = prompt_required("database")?;
    args.target = Some(target.clone());
    println!();
    Ok(target)
}

fn upgrade_snapshot(session: &Session) -> Result<UpgradeSnapshot> {
    let program_info = program_info(session)?;
    let storage = view(session, "storage_info", vec![])?;
    let auth = auth_info(session)?;
    let sqlite = query_typed(session, "select sqlite_version() as sqlite_version;")?;
    let sqlite_version =
        first_result_cell(&sqlite).ok_or_else(|| anyhow!("sqlite_version() returned no value"))?;
    let code_hash = program_info
        .get("code_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("program_info missing code_hash"))?
        .to_string();
    let code_bytes = program_info.get("code_bytes").and_then(json_u64_relaxed);
    let storage_generation = storage.get("generation").and_then(json_u64_relaxed);
    Ok(UpgradeSnapshot {
        program_info,
        storage,
        owner_sequence: auth.owner_sequence,
        auth,
        sqlite_version,
        code_hash,
        code_bytes,
        storage_generation,
    })
}

fn ensure_upgrade_owner(session: &Session, snapshot: &UpgradeSnapshot) -> Result<()> {
    match program_owner(&snapshot.program_info) {
        Some(owner) if owner == session.caller() => {}
        Some(owner) => bail!(
            "Circle owner is {owner}; current wallet {} cannot upgrade this database",
            session.caller()
        ),
        None => bail!("Circle program info did not expose an owner; refusing upgrade"),
    }
    if !snapshot.auth.configured {
        bail!("database is not OSW1 owner-write configured; refusing upgrade");
    }
    let owner_pubkey = snapshot
        .auth
        .owner_pubkey
        .as_deref()
        .ok_or_else(|| anyhow!("auth_info missing owner_pubkey"))?;
    let wallet_pubkey = hex::encode(session.intent_public_key()?);
    if owner_pubkey != wallet_pubkey {
        bail!("active wallet does not match the database OSW1 owner public key");
    }
    Ok(())
}

fn prepare_upgrade_wasm(auth: &AuthInfo) -> Result<PreparedUpgradeWasm> {
    let artifact = resolve_bundled_wasm_artifact()?;
    let mut bytes = artifact.bytes;
    let patch = patch_wasm_auth_from_info(&mut bytes, auth)?;
    let hash = sha256_hex(&bytes);
    Ok(PreparedUpgradeWasm {
        source: artifact.source,
        bytes,
        hash,
        patch,
    })
}

fn recover_live_wasm(
    session: &Session,
    before: &UpgradeSnapshot,
    previous_wasm: Option<&Path>,
) -> Result<Option<RecoveredWasm>> {
    let expected_hash = &before.code_hash;
    if let Some(path) = previous_wasm {
        return Ok(Some(recover_wasm_from_path(
            path,
            before,
            expected_hash,
            "operator_previous_wasm",
        )?));
    }

    for tx_hash in local_program_tx_candidates(session)? {
        if let Ok(tx) = transaction(session, &tx_hash)
            && let Some(bytes) = wasm_from_json(&tx, expected_hash)?
        {
            return Ok(Some(RecoveredWasm {
                bytes,
                hash: expected_hash.to_string(),
                source: "transaction".to_string(),
                tx_hash: Some(tx_hash),
            }));
        }
    }

    let mut offset = 0u64;
    while offset < 500 {
        let page = match transactions_by_address(session, &session.target().circle, 100, offset) {
            Ok(value) => value,
            Err(_) if offset == 0 => break,
            Err(error) => return Err(error.into()),
        };
        if let Some(bytes) = wasm_from_json(&page, expected_hash)? {
            return Ok(Some(RecoveredWasm {
                bytes,
                hash: expected_hash.to_string(),
                source: "address_history".to_string(),
                tx_hash: find_tx_hash_with_code(&page),
            }));
        }
        let count = page
            .as_array()
            .map(Vec::len)
            .or_else(|| {
                page.get("transactions")
                    .and_then(Value::as_array)
                    .map(Vec::len)
            })
            .or_else(|| page.get("txs").and_then(Value::as_array).map(Vec::len))
            .unwrap_or(0);
        if count < 100 {
            break;
        }
        offset += 100;
    }
    if let Some(recovered) = recover_wasm_from_local_base_artifacts(before, expected_hash)? {
        return Ok(Some(recovered));
    }
    if let Some(recovered) = recover_wasm_from_historical_catalog(before, expected_hash) {
        return Ok(Some(recovered));
    }
    Ok(None)
}

fn recover_wasm_from_path(
    path: &Path,
    before: &UpgradeSnapshot,
    expected_hash: &str,
    source: &str,
) -> Result<RecoveredWasm> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if sha256_hex(&bytes) == expected_hash {
        return Ok(RecoveredWasm {
            bytes,
            hash: expected_hash.to_string(),
            source: source.to_string(),
            tx_hash: None,
        });
    }
    let mut patched = bytes.clone();
    if patch_wasm_auth_from_info(&mut patched, &before.auth).is_ok()
        && sha256_hex(&patched) == expected_hash
    {
        return Ok(RecoveredWasm {
            bytes: patched,
            hash: expected_hash.to_string(),
            source: format!("{source}:personalized"),
            tx_hash: None,
        });
    }
    bail!(
        "{} does not match the current live program hash before or after OSW1 personalization",
        path.display()
    )
}

fn recover_wasm_from_local_base_artifacts(
    before: &UpgradeSnapshot,
    expected_hash: &str,
) -> Result<Option<RecoveredWasm>> {
    for path in local_wasm_artifact_candidates() {
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        if sha256_hex(&bytes) == expected_hash {
            return Ok(Some(RecoveredWasm {
                bytes,
                hash: expected_hash.to_string(),
                source: format!("local_artifact:{}", path.display()),
                tx_hash: None,
            }));
        }
        let mut patched = bytes;
        if patch_wasm_auth_from_info(&mut patched, &before.auth).is_ok()
            && sha256_hex(&patched) == expected_hash
        {
            return Ok(Some(RecoveredWasm {
                bytes: patched,
                hash: expected_hash.to_string(),
                source: format!("local_artifact_personalized:{}", path.display()),
                tx_hash: None,
            }));
        }
    }
    Ok(None)
}

fn recover_wasm_from_historical_catalog(
    before: &UpgradeSnapshot,
    expected_hash: &str,
) -> Option<RecoveredWasm> {
    let matched = match_historical_wasm(expected_hash, Some(&before.auth))?;
    Some(RecoveredWasm {
        bytes: matched.bytes,
        hash: expected_hash.to_string(),
        source: matched.source,
        tx_hash: None,
    })
}

fn local_wasm_artifact_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(path) = env::var("OCTRA_SQLITE_PREVIOUS_WASM") {
        push_unique_path(&mut paths, PathBuf::from(path));
    }
    if let Some(path) = find_project_file(DEFAULT_WASM_REL) {
        push_unique_path(&mut paths, path);
    }
    if let Ok(home) = env::var("HOME") {
        let registry = PathBuf::from(home)
            .join(".cargo")
            .join("registry")
            .join("src");
        collect_cargo_registry_wasms(&registry, &mut paths);
    }
    paths
}

fn collect_cargo_registry_wasms(root: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(registries) = fs::read_dir(root) else {
        return;
    };
    for registry in registries.flatten() {
        let Ok(crates) = fs::read_dir(registry.path()) else {
            continue;
        };
        for krate in crates.flatten() {
            let name = krate.file_name().to_string_lossy().to_string();
            if !name.starts_with("octra-sqlite-") {
                continue;
            }
            let path = krate.path().join(DEFAULT_WASM_REL);
            if path.is_file() {
                push_unique_path(paths, path);
            }
        }
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn local_program_tx_candidates(session: &Session) -> Result<Vec<String>> {
    let config = load_config().unwrap_or_default();
    let uri = canonical_database_uri(session.target());
    let mut candidates = Vec::new();
    for metadata in config.database_metadata.values().filter(|metadata| {
        metadata.uri == uri
            || (metadata.network == session.target().network
                && metadata.circle == session.target().circle)
    }) {
        if let Some(tx_hash) = metadata.program_update_tx.as_deref() {
            push_unique(&mut candidates, tx_hash);
        }
        if let Some(tx_hash) = metadata.create_tx.as_deref() {
            push_unique(&mut candidates, tx_hash);
        }
    }
    Ok(candidates)
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

pub(super) fn wasm_from_json(value: &Value, expected_hash: &str) -> Result<Option<Vec<u8>>> {
    match value {
        Value::Object(map) => {
            if let Some(code) = map.get("code_b64").and_then(Value::as_str) {
                let bytes = general_purpose::STANDARD
                    .decode(code)
                    .context("decoding code_b64 from transaction")?;
                if sha256_hex(&bytes) == expected_hash {
                    return Ok(Some(bytes));
                }
            }
            for value in map.values() {
                if let Some(bytes) = wasm_from_json(value, expected_hash)? {
                    return Ok(Some(bytes));
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                if let Some(bytes) = wasm_from_json(value, expected_hash)? {
                    return Ok(Some(bytes));
                }
            }
        }
        Value::String(text) if text.contains("code_b64") => {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                return wasm_from_json(&parsed, expected_hash);
            }
        }
        _ => {}
    }
    Ok(None)
}

pub(super) fn find_tx_hash_with_code(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            let contains_code = map.values().any(value_contains_code_b64);
            if contains_code
                && let Some(hash) = map
                    .get("tx_hash")
                    .or_else(|| map.get("hash"))
                    .and_then(Value::as_str)
            {
                return Some(hash.to_string());
            }
            map.values().find_map(find_tx_hash_with_code)
        }
        Value::Array(values) => values.iter().find_map(find_tx_hash_with_code),
        _ => None,
    }
}

fn value_contains_code_b64(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            map.contains_key("code_b64") || map.values().any(value_contains_code_b64)
        }
        Value::Array(values) => values.iter().any(value_contains_code_b64),
        Value::String(text) => text.contains("code_b64"),
        _ => false,
    }
}

fn default_or_requested_upgrade_bundle_paths(
    requested: Option<&Path>,
    session: &Session,
    before: &UpgradeSnapshot,
) -> Result<UpgradeBundlePaths> {
    let root = requested.map(Path::to_path_buf).unwrap_or(
        config_path()?
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("sqlite")
            .join("upgrades"),
    );
    let label = upgrade_bundle_label(
        &session.target().network,
        &session.target().circle,
        &before.sqlite_version,
        unix_seconds(),
    );
    let bundle_dir = unique_upgrade_bundle_dir(&root, &label);
    let file_stem = bundle_dir
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or(label);
    Ok(UpgradeBundlePaths {
        backup: bundle_dir.join(format!("{file_stem}.sqlite")),
        previous_wasm: bundle_dir.join("previous.wasm"),
        manifest: bundle_dir.join("upgrade.json"),
        bundle_dir,
    })
}

pub(super) fn upgrade_bundle_label(
    network: &str,
    circle: &str,
    sqlite_version: &str,
    timestamp: u64,
) -> String {
    format!(
        "{}-{}-sqlite-{}-{}",
        sanitize_path_component(network),
        sanitize_path_component(circle),
        sanitize_path_component(sqlite_version),
        utc_date_label(timestamp)
    )
}

fn unique_upgrade_bundle_dir(root: &Path, label: &str) -> PathBuf {
    let first = root.join(label);
    if !first.exists() {
        return first;
    }
    for index in 2..1000 {
        let candidate = root.join(format!("{label}-{index}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    root.join(format!("{label}-{}", unix_seconds()))
}

fn sanitize_path_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("restricting {}", path.display()))?;
    Ok(())
}

fn write_private_bytes(path: &Path, bytes: &[u8], create_new: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting {}", path.display()))?;
    Ok(())
}

fn write_private_json(path: &Path, value: &Value, create_new: bool) -> Result<()> {
    let text = serde_json::to_string_pretty(value)? + "\n";
    write_private_bytes(path, text.as_bytes(), create_new)
}

fn take_upgrade_backup(session: &Session, path: &Path, require_integrity: bool) -> Result<Value> {
    let summary = backup_database(session, path)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting {}", path.display()))?;
    let integrity = if sqlite3_available() {
        let result = run_local_sqlite_integrity(path)?;
        json!({
            "checked": true,
            "result": result,
        })
    } else if require_integrity {
        bail!("sqlite3 is required by --require-integrity but was not found on PATH");
    } else {
        json!({
            "checked": false,
            "reason": "sqlite3_not_found",
        })
    };
    Ok(json!({
        "skipped": false,
        "path": path.display().to_string(),
        "bytes": summary.bytes,
        "pages": summary.pages,
        "generation": summary.generation,
        "sha256": summary.sha256,
        "integrity": integrity,
    }))
}

fn sqlite3_available() -> bool {
    ProcessCommand::new("sqlite3")
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn ensure_upgrade_guard_unchanged(
    before: &UpgradeSnapshot,
    after: &UpgradeSnapshot,
    label: &str,
) -> Result<()> {
    if before.storage_generation != after.storage_generation {
        bail!(
            "{label} storage generation changed from {:?} to {:?}; another write may be in flight",
            before.storage_generation,
            after.storage_generation
        );
    }
    if before.owner_sequence != after.owner_sequence {
        bail!(
            "{label} owner sequence changed from {:?} to {:?}; another write may be in flight",
            before.owner_sequence,
            after.owner_sequence
        );
    }
    Ok(())
}

fn ensure_sqlite_version(snapshot: &UpgradeSnapshot, expected: &str) -> Result<()> {
    if snapshot.sqlite_version != expected {
        bail!(
            "sqlite_version() returned {}; expected {expected}",
            snapshot.sqlite_version
        );
    }
    Ok(())
}

fn optional_counter_unchanged(before: Option<u64>, after: Option<u64>) -> Option<bool> {
    Some(before? == after?)
}

fn clean_rollback_state(before: &UpgradeSnapshot, after: &UpgradeSnapshot) -> Option<bool> {
    let storage = optional_counter_unchanged(before.storage_generation, after.storage_generation);
    let owner = optional_counter_unchanged(before.owner_sequence, after.owner_sequence);
    if storage == Some(false) || owner == Some(false) {
        Some(false)
    } else if storage == Some(true) && owner == Some(true) {
        Some(true)
    } else {
        None
    }
}

fn rollback_clean_label(clean: Option<bool>) -> &'static str {
    match clean {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

fn submit_program_update(
    session: &Session,
    wasm: &[u8],
    ou: &str,
    expected_hash: &str,
) -> Result<ProgramUpdateOutcome> {
    let message = serde_json::to_string(&json!({
        "code_b64": general_purpose::STANDARD.encode(wasm),
    }))?;
    let tx = Tx {
        from: session.caller().to_string(),
        to_: session.target().circle.clone(),
        amount: "0".to_string(),
        nonce: next_nonce(session)?,
        ou: ou.to_string(),
        timestamp: now_timestamp(),
        op_type: "circle_program_update".to_string(),
        encrypted_data: String::new(),
        message,
        signature: String::new(),
        public_key: session.public_key_b64()?.to_string(),
    };
    let submitted = submit_tx(session, tx, true)?;
    let tx_hash = submitted
        .get("tx_hash")
        .and_then(Value::as_str)
        .map(str::to_string);
    let confirmation = if let Some(hash) = tx_hash.as_deref() {
        Some(wait_for_transaction(session, hash)?)
    } else {
        None
    };
    let program_info = wait_for_program_info(session, expected_hash)?;
    Ok(ProgramUpdateOutcome {
        submitted,
        tx_hash,
        confirmation,
        program_info,
    })
}

fn upgrade_plan_json(
    session: &Session,
    before: &UpgradeSnapshot,
    target_wasm: &PreparedUpgradeWasm,
    rollback: Option<&RecoveredWasm>,
    bundle_paths: Option<&UpgradeBundlePaths>,
    dry_run: bool,
    already_current: bool,
) -> Value {
    json!({
        "ok": true,
        "type": "upgrade",
        "schema": "octra-sqlite.cli.v1",
        "mode": if already_current { "already_current" } else if dry_run { "dry_run" } else { "planned" },
        "status": if already_current { "already_current" } else { "upgrade_needed" },
        "upgrade_required": !already_current,
        "dry_run": dry_run,
        "database": database_identity(session),
        "from": snapshot_program_json(before),
        "to": target_program_json(target_wasm),
        "rollback": if already_current { rollback_not_needed_json() } else { rollback_json(rollback, None, before, None) },
        "bundle": bundle_paths.map(|paths| bundle_json(&paths.bundle_dir, Some(&paths.manifest))).unwrap_or(Value::Null),
        "backup": bundle_paths.map(|paths| json!({
            "path": paths.backup.display().to_string(),
        })).unwrap_or(Value::Null),
    })
}

fn rollback_not_needed_json() -> Value {
    json!({
        "relevant": false,
        "available": false,
        "reason": "already_current",
    })
}

fn upgrade_result_json(input: UpgradeResultInput<'_>) -> Value {
    json!({
        "ok": true,
        "type": "upgrade",
        "schema": "octra-sqlite.cli.v1",
        "mode": "applied",
        "status": "applied",
        "upgrade_required": true,
        "dry_run": false,
        "database": database_identity(input.session),
        "from": snapshot_program_json(input.before),
        "to": snapshot_program_json(input.after),
        "target": target_program_json(input.target_wasm),
        "backup": input.backup,
        "rollback": rollback_json(
            input.rollback,
            input.previous_wasm_path,
            input.after,
            input.clean_rollback
        ),
        "transaction": program_update_json(input.session, input.update),
        "verification": {
            "sqlite_version": input.after.sqlite_version,
            "storage_generation_unchanged": optional_counter_unchanged(input.before.storage_generation, input.after.storage_generation),
            "owner_sequence_unchanged": optional_counter_unchanged(input.before.owner_sequence, input.after.owner_sequence),
        },
        "write_smoke": input.write_smoke,
        "metadata_saved": input.saved_metadata,
        "manifest": input.manifest,
    })
}

fn upgrade_manifest_json(input: UpgradeManifestInput<'_>) -> Value {
    json!({
        "schema": UPGRADE_BUNDLE_SCHEMA,
        "created_at_unix": unix_seconds(),
        "cli_version": env!("CARGO_PKG_VERSION"),
        "database": database_identity(input.session),
        "from": snapshot_program_json(input.before),
        "to": snapshot_program_json(input.after),
        "target": target_program_json(input.target_wasm),
        "backup": input.backup,
        "rollback": rollback_json(
            input.rollback,
            input.previous_wasm_path,
            input.after,
            input.clean_rollback,
        ),
        "transaction": program_update_json(input.session, input.update),
        "write_smoke": input.write_smoke,
        "metadata_saved": input.saved_metadata,
        "epoch": {
            "boundary": "circle_program_update",
            "from_code_hash": input.before.code_hash,
            "to_code_hash": input.after.code_hash,
            "tx_hash": input.update.tx_hash,
            "note": "Replay byte identity is per deployed engine epoch; keep this boundary with backups and traces."
        }
    })
}

fn snapshot_program_json(snapshot: &UpgradeSnapshot) -> Value {
    json!({
        "sqlite_version": snapshot.sqlite_version,
        "code_hash": snapshot.code_hash,
        "code_bytes": snapshot.code_bytes,
        "program_info": snapshot.program_info,
        "storage": {
            "generation": snapshot.storage_generation,
            "page_count": snapshot.storage.get("page_count").cloned().unwrap_or(Value::Null),
            "file_bytes": snapshot.storage.get("file_bytes").cloned().unwrap_or(Value::Null),
        },
        "auth": {
            "configured": snapshot.auth.configured,
            "owner_pubkey": snapshot.auth.owner_pubkey,
            "db_id": snapshot.auth.db_id,
            "owner_sequence": snapshot.owner_sequence,
        }
    })
}

fn target_program_json(target: &PreparedUpgradeWasm) -> Value {
    json!({
        "sqlite_version": SQLITE_VERSION,
        "code_hash": target.hash,
        "code_bytes": target.bytes.len(),
        "wasm": &target.source,
        "auth_patch": {
            "owner_pubkey": target.patch.owner_pubkey_hex,
            "db_id": target.patch.db_id_hex,
            "owner_pubkey_offset": target.patch.owner_pubkey_offset,
            "db_id_offset": target.patch.db_id_offset,
        }
    })
}

fn rollback_json(
    rollback: Option<&RecoveredWasm>,
    wasm_path: Option<&Path>,
    guard_snapshot: &UpgradeSnapshot,
    clean: Option<bool>,
) -> Value {
    match rollback {
        Some(recovered) => json!({
            "available": true,
            "code_hash": recovered.hash,
            "code_bytes": recovered.bytes.len(),
            "source": recovered.source,
            "tx_hash": recovered.tx_hash,
            "wasm": wasm_path.map(|path| path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string())),
            "clean": clean,
            "clean_reason": rollback_clean_reason(clean),
            "guard": {
                "storage_generation": guard_snapshot.storage_generation,
                "owner_sequence": guard_snapshot.owner_sequence,
            }
        }),
        None => json!({
            "available": false,
            "reason": "previous_wasm_not_recovered",
        }),
    }
}

fn rollback_clean_reason(clean: Option<bool>) -> Option<&'static str> {
    match clean {
        Some(true) => None,
        Some(false) => Some("counter_changed"),
        None => Some("counter_unknown"),
    }
}

fn bundle_json(bundle: &Path, manifest: Option<&Path>) -> Value {
    json!({
        "path": bundle.display().to_string(),
        "manifest": manifest.map(|path| path.display().to_string()),
    })
}

fn program_update_json(session: &Session, update: &ProgramUpdateOutcome) -> Value {
    json!({
        "tx_hash": update.tx_hash,
        "tx_url": update
            .tx_hash
            .as_deref()
            .and_then(|hash| explorer_tx_url(&session.target().network, hash)),
        "submitted": update.submitted,
        "confirmation": update.confirmation.clone().map(redact_code_payload),
        "program_info": update.program_info,
    })
}

fn prompt_upgrade_wizard(
    args: &mut UpgradeArgs,
    plan: &Value,
    paths: Option<&UpgradeBundlePaths>,
) -> Result<()> {
    print_title("Upgrade");
    print_upgrade_plan(plan);
    if let Some(paths) = paths {
        print_field("bundle", paths.bundle_dir.display().to_string());
        print_field("backup", paths.backup.display().to_string());
    }
    println!();
    print_section("Safety");
    print_field("owner", "current wallet owns the Circle and OSW1 database");
    if plan
        .pointer("/rollback/available")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        print_field("rollback", "previous program recovered");
    } else {
        print_field("rollback", "not available");
    }
    if sqlite3_available() {
        print_field("backup integrity", "will run local sqlite3 integrity_check");
    } else if args.require_integrity {
        print_field("backup integrity", "required; sqlite3 must be on PATH");
    } else {
        print_field(
            "backup integrity",
            "sqlite3 not found; backup still written",
        );
    }
    if args.write_smoke {
        print_warning(
            "--write-smoke performs an owner-signed write after upgrade and will block clean rollback",
        );
    }
    println!();

    if !args.skip_backup && !prompt_yes_no("write local backup before upgrade", true)? {
        print_warning("skipping backup removes the local SQLite recovery snapshot");
        if !prompt_yes_no("continue without local backup", false)? {
            bail!("upgrade cancelled");
        }
        args.skip_backup = true;
    }

    if !args.write_smoke && prompt_yes_no("run write smoke after upgrade", false)? {
        print_warning(
            "write smoke dirties the database and makes rollback require --force-after-writes",
        );
        args.write_smoke = true;
    }

    if !prompt_yes_no("upgrade this database now", false)? {
        bail!("upgrade cancelled");
    }
    Ok(())
}

fn print_upgrade_plan(plan: &Value) {
    if let Some(database) = plan.pointer("/database/uri").and_then(Value::as_str) {
        print_field("database", database);
    }
    if let Some(mode) = plan.get("mode").and_then(Value::as_str) {
        print_field("mode", mode);
    }
    let from_version = plan
        .pointer("/from/sqlite_version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let to_version = plan
        .pointer("/to/sqlite_version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    print_field("sqlite", format!("{from_version} -> {to_version}"));
    if let Some(from_hash) = plan.pointer("/from/code_hash").and_then(Value::as_str)
        && let Some(to_hash) = plan.pointer("/to/code_hash").and_then(Value::as_str)
    {
        print_field("program hash", format!("{from_hash} -> {to_hash}"));
    }
    if plan.pointer("/rollback/relevant").and_then(Value::as_bool) == Some(false) {
        print_field("rollback", "not needed");
    } else if let Some(available) = plan.pointer("/rollback/available").and_then(Value::as_bool) {
        print_field("rollback available", available.to_string());
    }
    if let Some(path) = plan.pointer("/bundle/path").and_then(Value::as_str) {
        print_field("bundle", path);
    }
    if let Some(path) = plan.pointer("/backup/path").and_then(Value::as_str) {
        print_field("backup", path);
    }
}

fn json_u64_relaxed(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn utc_date_label(timestamp: u64) -> String {
    let days = (timestamp / 86_400) as i64;
    let (year, month, day) = civil_from_unix_days(days);
    format!("{year:04}{month:02}{day:02}")
}

fn civil_from_unix_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previous_base_wasm_is_personalized_for_rollback_recovery() {
        let owner_pubkey = [7u8; 32];
        let db_id = [9u8; 32];
        let mut base = Vec::new();
        base.extend_from_slice(b"before");
        base.extend_from_slice(OWNER_PUBKEY_PLACEHOLDER);
        base.extend_from_slice(b"middle");
        base.extend_from_slice(DB_ID_PLACEHOLDER);
        base.extend_from_slice(b"after");

        let mut personalized = base.clone();
        patch_wasm_auth_bytes(&mut personalized, &owner_pubkey, &db_id).unwrap();
        let expected_hash = sha256_hex(&personalized);

        let path = env::temp_dir().join(format!(
            "octra-sqlite-previous-base-{}.wasm",
            std::process::id()
        ));
        fs::write(&path, &base).unwrap();

        let snapshot = UpgradeSnapshot {
            program_info: json!({}),
            storage: json!({}),
            auth: AuthInfo {
                configured: true,
                db_id: hex::encode(db_id),
                owner_pubkey: Some(hex::encode(owner_pubkey)),
                owner_sequence: Some(1),
            },
            sqlite_version: "3.53.2".to_string(),
            code_hash: expected_hash.clone(),
            code_bytes: Some(personalized.len() as u64),
            storage_generation: Some(1),
            owner_sequence: Some(1),
        };

        let recovered =
            recover_wasm_from_path(&path, &snapshot, &expected_hash, "test_previous").unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(recovered.bytes, personalized);
        assert_eq!(recovered.hash, expected_hash);
        assert_eq!(recovered.source, "test_previous:personalized");
    }

    #[test]
    fn historical_catalog_wasm_is_personalized_for_rollback_recovery() {
        let owner_pubkey = [7u8; 32];
        let db_id = [9u8; 32];
        let artifact = HISTORICAL_WASMS
            .iter()
            .find(|artifact| artifact.releases == "0.3.0")
            .unwrap();
        let mut personalized = artifact.bytes.to_vec();
        patch_wasm_auth_bytes(&mut personalized, &owner_pubkey, &db_id).unwrap();
        let expected_hash = sha256_hex(&personalized);
        let snapshot = UpgradeSnapshot {
            program_info: json!({}),
            storage: json!({}),
            auth: AuthInfo {
                configured: true,
                db_id: hex::encode(db_id),
                owner_pubkey: Some(hex::encode(owner_pubkey)),
                owner_sequence: Some(1),
            },
            sqlite_version: "3.53.2".to_string(),
            code_hash: expected_hash.clone(),
            code_bytes: Some(personalized.len() as u64),
            storage_generation: Some(1),
            owner_sequence: Some(1),
        };

        let recovered = recover_wasm_from_historical_catalog(&snapshot, &expected_hash)
            .expect("historical recovery");

        assert_eq!(recovered.bytes, personalized);
        assert_eq!(recovered.hash, expected_hash);
        assert_eq!(recovered.source, "historical_release:0.3.0:personalized");
    }

    #[test]
    fn optional_counter_comparison_is_unknown_when_missing() {
        assert_eq!(optional_counter_unchanged(Some(1), Some(1)), Some(true));
        assert_eq!(optional_counter_unchanged(Some(1), Some(2)), Some(false));
        assert_eq!(optional_counter_unchanged(Some(1), None), None);
        assert_eq!(optional_counter_unchanged(None, Some(1)), None);
        assert_eq!(optional_counter_unchanged(None, None), None);
    }

    #[test]
    fn clean_rollback_is_unknown_when_counter_is_missing() {
        let snapshot = |storage_generation, owner_sequence| UpgradeSnapshot {
            program_info: json!({}),
            storage: json!({}),
            auth: AuthInfo {
                configured: true,
                db_id: "00".repeat(32),
                owner_pubkey: Some("00".repeat(32)),
                owner_sequence,
            },
            sqlite_version: "3.53.2".to_string(),
            code_hash: "hash".to_string(),
            code_bytes: Some(1),
            storage_generation,
            owner_sequence,
        };

        assert_eq!(
            clean_rollback_state(&snapshot(Some(1), Some(1)), &snapshot(Some(1), Some(1))),
            Some(true)
        );
        assert_eq!(
            clean_rollback_state(&snapshot(Some(1), Some(1)), &snapshot(Some(2), Some(1))),
            Some(false)
        );
        assert_eq!(
            clean_rollback_state(&snapshot(Some(1), Some(1)), &snapshot(Some(1), None)),
            None
        );
    }

    #[test]
    fn already_current_rollback_is_not_relevant() {
        let rollback = rollback_not_needed_json();
        assert_eq!(rollback["relevant"], false);
        assert_eq!(rollback["available"], false);
        assert_eq!(rollback["reason"], "already_current");
    }
}
