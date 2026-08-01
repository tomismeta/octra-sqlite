use super::*;

pub(super) struct MatchedWasmArtifact {
    pub(super) releases: String,
    pub(super) sqlite_version: String,
    pub(super) sha256: String,
    pub(super) bytes: u64,
    pub(super) source_url: String,
    pub(super) exact_hash: bool,
}

pub(super) struct VerifyWriteSmoke {
    create: Value,
    rows: Value,
    cleanup: Value,
}

pub(super) struct VerifyIntegrity {
    summary: BackupSummary,
    result: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HistoricalWasmMetadata {
    pub(super) releases: String,
    pub(super) sqlite_version: String,
    pub(super) sha256: String,
    pub(super) bytes: u64,
    pub(super) source_url: String,
}

pub(super) fn cmd_status(args: StatusArgs, label: &str) -> Result<i32> {
    let mut report = StatusReport::new(label, args.json);
    report.init_database_readiness();
    let config_path = config_path()?;
    let explicit_target = args.target.target.is_some();
    match load_config() {
        Ok(config) => {
            let explicit_target_unsigned_read =
                explicit_target_allows_unsigned_read(&args.target, &config);
            if config_path.exists() {
                report.ok("config", format!("read {}", config_path.display()));
            } else if explicit_target {
                report.ok(
                    "config",
                    format!(
                        "not found at {}; explicit database target supplied",
                        config_path.display()
                    ),
                );
            } else {
                report.warn(
                    "config",
                    format!(
                        "not found at {}; run octra-sqlite setup",
                        config_path.display()
                    ),
                );
            }
            if let Some(default_database) = &config.default_database {
                report.ok("default database", default_database);
            } else if explicit_target {
                report.ok(
                    "default database",
                    "not needed for explicit database target",
                );
            } else {
                report.warn(
                    "default database",
                    "not set; run octra-sqlite database default DATABASE or pass a database argument",
                );
            }

            let wallet_path = resolve_wallet_path(&args.target, &config);
            match wallet_caller(wallet_path.as_deref(), args.target.caller.as_deref()) {
                Ok(caller) => {
                    if let Some(path) = wallet_path {
                        report.ok("wallet", format!("read {}", path.display()));
                    } else if env::var("OCTRA_PRIVATE_KEY_B64").is_ok() {
                        report.ok("wallet", "using OCTRA_PRIVATE_KEY_B64");
                    } else if explicit_target_unsigned_read {
                        report.ok(
                            "wallet",
                            "not configured; public reads can continue without a wallet",
                        );
                    } else {
                        report.warn(
                            "wallet",
                            "not configured; sealed reads and writes need signed RPC",
                        );
                    }
                    if let Some(caller) = caller {
                        report.ok("caller", caller);
                    } else if explicit_target_unsigned_read {
                        report.ok("caller", "not needed for public reads");
                    } else {
                        report.warn("caller", "not found in wallet/env");
                    }
                }
                Err(error) if explicit_target_unsigned_read => {
                    report.warn(
                        "wallet",
                        format!("failed to load; public reads can continue without it: {error}"),
                    );
                    report.ok("caller", "not needed for public reads");
                }
                Err(error) => report.fail("wallet", error.to_string()),
            }

            check_release_manifest(&mut report);
            check_bundled_wasm(&mut report);

            if args.skip_network {
                report.warn("network", "skipped by --skip-network");
            } else {
                match build_session(&args.target) {
                    Ok(session) => check_live_target(
                        &mut report,
                        &session,
                        args.expected_hash
                            .as_deref()
                            .unwrap_or(EXPECTED_WASM_SHA256),
                    ),
                    Err(error) => report.warn(
                        "network",
                        format!("skipped; could not build signed session: {error:#}"),
                    ),
                }
            }
        }
        Err(error) => {
            report.fail("config", error.to_string());
            check_release_manifest(&mut report);
            check_bundled_wasm(&mut report);
        }
    }
    report.finish_with_ready(label, args.ready)
}

pub(super) fn cmd_config(args: ConfigArgs) -> Result<()> {
    let config = load_config()?;
    let path = config_path()?;
    if args.json {
        return print_json(&json!({
            "config": path,
            "wallet": config.wallet,
            "network": config.network,
            "rpc": config.rpc,
            "explorer": config.explorer,
            "networks": config.networks,
            "default_database": config.default_database,
            "databases": config.databases,
            "database_metadata": config.database_metadata,
        }));
    }

    print_field("config", path.display().to_string());
    print_field(
        "wallet",
        config.wallet.as_deref().unwrap_or("(not configured)"),
    );
    print_field(
        "network",
        config.network.as_deref().unwrap_or("(not configured)"),
    );
    print_field("rpc", config.rpc.as_deref().unwrap_or("(not configured)"));
    print_field(
        "explorer",
        config.explorer.as_deref().unwrap_or("(not configured)"),
    );
    print_field(
        "default database",
        config
            .default_database
            .as_deref()
            .unwrap_or("(not configured)"),
    );
    if !config.networks.is_empty() {
        println!("{}", dim("networks:"));
        for (name, profile) in &config.networks {
            println!(
                "  {name}: rpc {}, explorer {}",
                profile.rpc.as_deref().unwrap_or("(not configured)"),
                profile.explorer.as_deref().unwrap_or("(not configured)")
            );
        }
    }
    print_field("databases", config.databases.len().to_string());
    if !config.databases.is_empty() {
        print_field("next", "octra-sqlite database list");
    } else {
        print_field("create", CREATE_DATABASE_COMMAND);
    }
    Ok(())
}

pub(super) fn report_wallet_permissions(report: &mut StatusReport, path: &Path) {
    match fs::metadata(path) {
        Ok(metadata) => {
            #[cfg(unix)]
            {
                let mode = metadata.permissions().mode() & 0o777;
                let rendered = format!("{mode:04o}");
                if mode & 0o077 == 0 {
                    report.ok("wallet permissions", rendered);
                } else {
                    report.warn(
                        "wallet permissions",
                        format!("{rendered}; recommended 0600 or 0640"),
                    );
                }
            }
            #[cfg(not(unix))]
            {
                let readonly = metadata.permissions().readonly();
                report.ok(
                    "wallet permissions",
                    if readonly { "readonly" } else { "writable" },
                );
            }
        }
        Err(error) => report.warn("wallet permissions", error.to_string()),
    }
}

pub(super) struct StatusReport {
    label: String,
    json: bool,
    failures: usize,
    warnings: usize,
    pub(super) sqlite_version: Option<String>,
    pub(super) program_version: Option<String>,
    pub(super) engine_current: Option<bool>,
    items: Vec<Value>,
    readiness: Map<String, Value>,
}

impl StatusReport {
    pub(super) fn new(label: &str, json: bool) -> Self {
        Self {
            label: label.to_string(),
            json,
            failures: 0,
            warnings: 0,
            sqlite_version: None,
            program_version: None,
            engine_current: None,
            items: Vec::new(),
            readiness: Map::new(),
        }
    }

    pub(super) fn ok(&mut self, label: &str, detail: impl AsRef<str>) {
        self.record("ok", label, detail);
    }

    pub(super) fn warn(&mut self, label: &str, detail: impl AsRef<str>) {
        self.warnings += 1;
        self.record("warn", label, detail);
    }

    pub(super) fn fail(&mut self, label: &str, detail: impl AsRef<str>) {
        self.failures += 1;
        self.record("fail", label, detail);
    }

    pub(super) fn record(&mut self, status: &str, label: &str, detail: impl AsRef<str>) {
        let detail = detail.as_ref().to_string();
        self.items.push(json!({
            "status": status,
            "label": label,
            "detail": detail,
        }));
        if !self.json {
            print!("{}", format_status_line(status, label, &detail));
        }
    }

    pub(super) fn ready(&mut self, key: &str, ready: bool) {
        self.readiness.insert(key.to_string(), Value::Bool(ready));
    }

    pub(super) fn engine_current(&mut self, current: bool) {
        self.engine_current = Some(current);
    }

    pub(super) fn program_version(&mut self, version: impl Into<String>) {
        self.program_version = Some(version.into());
    }

    pub(super) fn sqlite_version(&mut self, version: impl Into<String>) {
        self.sqlite_version = Some(version.into());
    }

    pub(super) fn init_database_readiness(&mut self) {
        for key in DATABASE_READINESS_KEYS {
            self.readiness.insert(key.to_string(), Value::Null);
        }
    }

    pub(super) fn finish(self, label: &str) -> Result<()> {
        self.finish_with_ready(label, false).map(|_| ())
    }

    pub(super) fn finish_with_ready(self, label: &str, require_ready: bool) -> Result<i32> {
        let read_ready = self.read_ready();
        let write_ready = self.write_ready();
        let ok = self.failures == 0 && (!require_ready || read_ready);
        let upgrade_needed = self.engine_current.map(|current| !current);
        if self.json {
            return print_json(&self.into_json_value(ok, read_ready, write_ready, upgrade_needed))
                .map(|_| if ok { 0 } else { 1 });
        }
        if self.failures != 0 {
            bail!("{label} found {} issue(s)", self.failures)
        } else if require_ready && !read_ready {
            println!("{}", format_status_line("fail", "read_ready", "false"));
            Ok(1)
        } else {
            let upgrade_suffix = upgrade_needed
                .filter(|needed| *needed)
                .map(|_| " upgrade_needed=true")
                .unwrap_or("");
            println!(
                "{} read_ready={} write_ready={}{}",
                dim(format!("{label}:")),
                read_ready,
                write_ready,
                upgrade_suffix
            );
            Ok(0)
        }
    }

    pub(super) fn into_json_value(
        self,
        ok: bool,
        read_ready: bool,
        write_ready: bool,
        upgrade_needed: Option<bool>,
    ) -> Value {
        let mut readiness = self.readiness;
        readiness.insert("read_ready".to_string(), Value::Bool(read_ready));
        readiness.insert("write_ready".to_string(), Value::Bool(write_ready));
        json!({
            "ok": ok,
            "type": self.label,
            "schema": "octra-sqlite.cli.v1",
            "ready": read_ready,
            "read_ready": read_ready,
            "write_ready": write_ready,
            "sqlite_version": self.sqlite_version,
            "program_version": self.program_version,
            "engine_current": self.engine_current,
            "upgrade_needed": upgrade_needed,
            "failures": self.failures,
            "warnings": self.warnings,
            "readiness": readiness,
            "items": self.items,
        })
    }

    pub(super) fn read_ready(&self) -> bool {
        READ_READINESS_KEYS
            .iter()
            .all(|key| self.readiness.get(*key).and_then(Value::as_bool) == Some(true))
    }

    pub(super) fn write_ready(&self) -> bool {
        WRITE_READINESS_KEYS
            .iter()
            .all(|key| self.readiness.get(*key).and_then(Value::as_bool) == Some(true))
    }
}

pub(super) const DATABASE_READINESS_KEYS: &[&str] = &[
    "circle_reachable",
    "auth_readable",
    "owner_write_valid",
    "storage_initialized",
    "sqlite_ready",
    "query_ready",
];

pub(super) const READ_READINESS_KEYS: &[&str] = &[
    "circle_reachable",
    "auth_readable",
    "storage_initialized",
    "sqlite_ready",
    "query_ready",
];

pub(super) const WRITE_READINESS_KEYS: &[&str] =
    &["circle_reachable", "auth_readable", "owner_write_valid"];

pub(super) fn check_bundled_wasm(report: &mut StatusReport) {
    match resolve_bundled_wasm_artifact() {
        Ok(artifact) => {
            let hash = sha256_hex(&artifact.bytes);
            if artifact.bytes.len() == EXPECTED_WASM_BYTES {
                report.ok(
                    "wasm bytes",
                    format!("{} bytes ({})", artifact.bytes.len(), artifact.source),
                );
            } else {
                report.fail(
                    "wasm bytes",
                    format!(
                        "{} bytes at {}; expected {}",
                        artifact.bytes.len(),
                        artifact.source,
                        EXPECTED_WASM_BYTES
                    ),
                );
            }
            if hash == EXPECTED_WASM_SHA256 {
                report.ok("wasm sha256", hash);
            } else {
                report.fail(
                    "wasm sha256",
                    format!(
                        "{hash} at {}; expected {EXPECTED_WASM_SHA256}",
                        artifact.source
                    ),
                );
            }
        }
        Err(error) => report.fail("wasm", error.to_string()),
    }
}

pub(super) fn check_release_manifest(report: &mut StatusReport) {
    let artifact = match resolve_release_manifest() {
        Ok(artifact) => artifact,
        Err(error) => {
            report.fail("release manifest", error.to_string());
            return;
        }
    };
    let manifest: Value = match serde_json::from_str(&artifact.text) {
        Ok(value) => value,
        Err(error) => {
            report.fail(
                "release manifest",
                format!("parsing {}: {error}", artifact.source),
            );
            return;
        }
    };
    report.ok("release manifest", artifact.source);
    let manifest_hash = manifest
        .pointer("/wasm/sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let manifest_bytes = manifest
        .pointer("/wasm/bytes")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if manifest_hash == EXPECTED_WASM_SHA256 {
        report.ok("manifest wasm hash", manifest_hash);
    } else {
        report.fail(
            "manifest wasm hash",
            format!("{manifest_hash}; expected {EXPECTED_WASM_SHA256}"),
        );
    }
    if manifest_bytes == EXPECTED_WASM_BYTES as u64 {
        report.ok("manifest wasm bytes", manifest_bytes.to_string());
    } else {
        report.fail(
            "manifest wasm bytes",
            format!("{manifest_bytes}; expected {EXPECTED_WASM_BYTES}"),
        );
    }
}

pub(super) fn check_live_target(report: &mut StatusReport, session: &Session, expected_hash: &str) {
    report.ok("rpc", session.rpc());
    report.ok("read_mode", session.target().read_mode.as_str());
    if let Some(url) = explorer_circle_url(&session.target().network, &session.target().circle) {
        report.ok("explorer", url);
    }
    match program_info(session) {
        Ok(info) => {
            report.ready("circle_reachable", true);
            report.ok(
                "circle",
                linked_circle(&session.target().network, &session.target().circle),
            );
            let program_version = program_version_string(&info);
            if let Some(version) = program_version.clone() {
                report.program_version(version);
            }
            let version = program_version.as_deref().unwrap_or("unknown");
            let code_hash = info
                .get("code_hash")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let code_bytes = info
                .get("code_bytes")
                .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()));
            report.ok("program version", version);
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
            if code_hash == expected_hash {
                report.ok("program hash", code_hash);
                report.engine_current(true);
            } else if expected_hash == EXPECTED_WASM_SHA256 {
                match personalized_wasm_hash(session) {
                    Ok(Some(personalized_hash)) if code_hash == personalized_hash => {
                        report.ok(
                            "program hash",
                            format!("{code_hash} (owner-personalized bundled WASM)"),
                        );
                        report.engine_current(true);
                    }
                    Ok(Some(personalized_hash)) => match match_historical_wasm(code_hash, code_bytes)
                    {
                        Some(match_) => {
                            report.engine_current(false);
                            report.warn(
                                "program hash",
                                render_historical_wasm_match(code_hash, &match_),
                            );
                        }
                        None => report.fail(
                            "program hash",
                            format!(
                                "{code_hash}; expected {expected_hash} or owner-personalized {personalized_hash}"
                            ),
                        ),
                    },
                    Ok(None) => match match_historical_wasm(code_hash, code_bytes) {
                        Some(match_) => {
                            report.engine_current(false);
                            report.warn(
                                "program hash",
                                render_historical_wasm_match(code_hash, &match_),
                            );
                        }
                        None => report.fail(
                            "program hash",
                            format!("{code_hash}; expected {expected_hash}"),
                        ),
                    },
                    Err(error) => match match_historical_wasm(code_hash, code_bytes) {
                        Some(match_) => {
                            report.engine_current(false);
                            report.warn(
                                "program hash",
                                render_historical_wasm_match(code_hash, &match_),
                            );
                        }
                        None => report.fail(
                            "program hash",
                            format!("{code_hash}; expected {expected_hash}; personalized check failed: {error:#}"),
                        ),
                    },
                }
            } else {
                report.fail(
                    "program hash",
                    format!("{code_hash}; expected {expected_hash}"),
                );
            }
        }
        Err(error) => {
            if session.target().read_mode.allows_unsigned_read() {
                match circle_info(session) {
                    Ok(info) => {
                        report.ready("circle_reachable", true);
                        if circle_info_allows_unsigned_read(&info) {
                            report.ok(
                                "program info",
                                format!(
                                    "using Circle metadata; signed program info unavailable: {error}"
                                ),
                            );
                        } else {
                            report.fail(
                                "program info",
                                format!(
                                    "signed program info unavailable: {error}; Circle metadata is not public-read"
                                ),
                            );
                        }
                        report.ok(
                            "circle",
                            linked_circle(&session.target().network, &session.target().circle),
                        );
                        if let Some(privacy_class) =
                            info.get("privacy_class").and_then(Value::as_str)
                        {
                            report.ok("privacy_class", privacy_class);
                        }
                        if let Some(browser_mode) = info.get("browser_mode").and_then(Value::as_str)
                        {
                            report.ok("browser_mode", browser_mode);
                        }
                        if let Some(resource_mode) =
                            info.get("resource_mode").and_then(Value::as_str)
                        {
                            report.ok("resource_mode", resource_mode);
                        }
                    }
                    Err(info_error) => {
                        report.ready("circle_reachable", false);
                        report.fail(
                            "program info",
                            format!("{error}; unsigned circle info failed: {info_error}"),
                        );
                    }
                }
            } else {
                report.ready("circle_reachable", false);
                report.fail("program info", error.to_string());
            }
        }
    }
    match view(session, "storage_info", vec![]) {
        Ok(storage) => {
            let page_count = storage
                .get("page_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            report.ready("storage_initialized", page_count > 0);
            report.ok(
                "storage",
                format!(
                    "{} pages, {} bytes",
                    storage
                        .get("page_count")
                        .map(value_to_string)
                        .unwrap_or_else(|| "?".to_string()),
                    storage
                        .get("file_bytes")
                        .map(value_to_string)
                        .unwrap_or_else(|| "?".to_string())
                ),
            );
        }
        Err(error) => {
            report.ready("storage_initialized", false);
            report.fail("storage", error.to_string());
        }
    }
    match auth_info(session) {
        Ok(auth) => {
            report.ready("auth_readable", true);
            if auth.configured {
                report.ok("auth", "OSW1 owner write intent");
                if let Some(owner_pubkey) = auth.owner_pubkey.as_deref() {
                    report.ok("auth owner pubkey", owner_pubkey);
                    match session.intent_public_key() {
                        Ok(wallet_pubkey) if hex::encode(wallet_pubkey) == owner_pubkey => {
                            report.ready("owner_write_valid", true);
                            report.ok("auth owner wallet", "current wallet can write")
                        }
                        Ok(_) => {
                            report.ready("owner_write_valid", false);
                            report.warn("auth owner wallet", "current wallet is read-only")
                        }
                        Err(error) => {
                            report.ready("owner_write_valid", false);
                            if session.wallet_load_error().is_some() {
                                let _ = error;
                                report.ok(
                                    "auth owner wallet",
                                    "wallet failed to load; writes require a valid owner wallet",
                                );
                            } else if session.target().read_mode.allows_unsigned_read() {
                                let _ = error;
                                report.ok(
                                    "auth owner wallet",
                                    "not configured; writes require the owner wallet",
                                );
                            } else {
                                report.warn(
                                    "auth owner wallet",
                                    format!("could not derive wallet public key: {error:#}"),
                                );
                            }
                        }
                    }
                }
                report.ok("auth db id", &auth.db_id);
                if let Some(sequence) = auth.owner_sequence {
                    report.ok("auth sequence", sequence.to_string());
                }
            } else {
                report.ready("owner_write_valid", false);
                report.warn("auth", "unconfigured bundled WASM; writes are unsigned");
            }
        }
        Err(error) => {
            report.ready("auth_readable", false);
            report.ready("owner_write_valid", false);
            report.fail("auth info", error.to_string());
        }
    }
    match query_typed(session, "select sqlite_version() as sqlite_version;") {
        Ok(result) => match first_result_string(&result) {
            Some(sqlite_version) => {
                report.ready("sqlite_ready", true);
                report.ready("query_ready", true);
                report.sqlite_version(sqlite_version.clone());
                report.ok("sqlite version", sqlite_version);
            }
            None => {
                report.ready("sqlite_ready", false);
                report.ready("query_ready", false);
                report.fail(
                    "sqlite version",
                    "sqlite_version() returned no string value",
                );
            }
        },
        Err(error) => {
            report.ready("sqlite_ready", false);
            report.ready("query_ready", false);
            report.fail("sqlite version", error.to_string());
        }
    }
}

pub(super) fn first_result_cell(result: &Value) -> Option<String> {
    result
        .get("rows")?
        .as_array()?
        .first()?
        .as_array()?
        .first()
        .map(value_to_string)
}

pub(super) fn first_result_string(result: &Value) -> Option<String> {
    result
        .get("rows")?
        .as_array()?
        .first()?
        .as_array()?
        .first()?
        .as_str()
        .map(str::to_string)
}

pub(super) fn program_version_string(info: &Value) -> Option<String> {
    info.get("version")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn program_owner(info: &Value) -> Option<&str> {
    ["owner", "program_owner", "creator", "deployer"]
        .into_iter()
        .find_map(|key| info.get(key).and_then(Value::as_str))
}

pub(super) fn personalized_wasm_hash(session: &Session) -> Result<Option<String>> {
    let auth = auth_info(session)?;
    if !auth.configured {
        return Ok(None);
    }
    let owner_pubkey = hex_to_32(
        "owner_pubkey",
        auth.owner_pubkey
            .as_deref()
            .ok_or_else(|| auth_info_error("auth_info missing owner_pubkey"))?,
    )?;
    let db_id = hex_to_32("db_id", &auth.db_id)?;
    let artifact = resolve_bundled_wasm_artifact()?;
    let mut wasm = artifact.bytes;
    patch_wasm_auth_bytes(&mut wasm, &owner_pubkey, &db_id)?;
    Ok(Some(sha256_hex(&wasm)))
}

pub(super) fn match_historical_wasm(
    expected_hash: &str,
    code_bytes: Option<u64>,
) -> Option<MatchedWasmArtifact> {
    let catalog = historical_wasm_catalog().ok()?;
    match_historical_wasm_in_catalog(&catalog, expected_hash, code_bytes)
}

pub(super) fn match_historical_wasm_in_catalog(
    catalog: &[HistoricalWasmMetadata],
    expected_hash: &str,
    code_bytes: Option<u64>,
) -> Option<MatchedWasmArtifact> {
    for artifact in catalog {
        if expected_hash == artifact.sha256 {
            return Some(MatchedWasmArtifact {
                releases: artifact.releases.clone(),
                sqlite_version: artifact.sqlite_version.clone(),
                sha256: artifact.sha256.clone(),
                bytes: artifact.bytes,
                source_url: artifact.source_url.clone(),
                exact_hash: true,
            });
        }
    }
    if let Some(code_bytes) = code_bytes
        && let Some(artifact) = catalog.iter().find(|artifact| artifact.bytes == code_bytes)
    {
        return Some(MatchedWasmArtifact {
            releases: artifact.releases.clone(),
            sqlite_version: artifact.sqlite_version.clone(),
            sha256: artifact.sha256.clone(),
            bytes: artifact.bytes,
            source_url: artifact.source_url.clone(),
            exact_hash: false,
        });
    }
    None
}

pub(super) fn historical_wasm_catalog() -> Result<Vec<HistoricalWasmMetadata>> {
    let artifact = resolve_release_manifest()?;
    let manifest: Value = serde_json::from_str(&artifact.text)
        .with_context(|| format!("parsing {}", artifact.source))?;
    parse_historical_wasm_catalog(&manifest)
}

pub(super) fn parse_historical_wasm_catalog(
    manifest: &Value,
) -> Result<Vec<HistoricalWasmMetadata>> {
    let entries = manifest
        .get("historical_wasm_catalog")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("release manifest missing historical_wasm_catalog array"))?;
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| parse_historical_wasm_entry(index, entry))
        .collect()
}

pub(super) fn parse_historical_wasm_entry(
    index: usize,
    entry: &Value,
) -> Result<HistoricalWasmMetadata> {
    let field = |name: &str| format!("historical_wasm_catalog[{index}].{name}");
    Ok(HistoricalWasmMetadata {
        releases: entry
            .get("releases")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{} missing or not a string", field("releases")))?
            .to_string(),
        sqlite_version: entry
            .get("sqlite_version")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{} missing or not a string", field("sqlite_version")))?
            .to_string(),
        sha256: entry
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{} missing or not a string", field("sha256")))?
            .to_string(),
        bytes: entry
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("{} missing or not an unsigned integer", field("bytes")))?,
        source_url: entry
            .get("source_url")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{} missing or not a string", field("source_url")))?
            .to_string(),
    })
}

pub(super) fn render_historical_wasm_match(
    code_hash: &str,
    match_: &MatchedWasmArtifact,
) -> String {
    let confidence = if match_.exact_hash {
        "known historical WASM"
    } else {
        "possible historical WASM byte-size match"
    };
    format!(
        "{code_hash} ({confidence} {}, SQLite {}; upgrade available; rollback source {})",
        match_.releases, match_.sqlite_version, match_.source_url
    )
}

pub(super) fn cmd_receipt(args: ReceiptArgs) -> Result<i32> {
    let session = build_session(&args.target)?;
    let receipt = wait_for_receipt(&session, &args.tx_hash).map_err(|error| {
        if error.kind() == ErrorKind::Timeout {
            let database = canonical_database_uri(session.target());
            let next_command = format!("octra-sqlite receipt {} {database} --json", args.tx_hash);
            Error::with_code_and_details(
                ErrorKind::Timeout,
                "receipt_pending",
                format!(
                    "receipt is still pending; tx_hash={}; circle={}; next=\"{next_command}\"",
                    args.tx_hash,
                    session.target().circle
                ),
                [
                    ("tx_hash", Value::String(args.tx_hash.clone())),
                    ("circle", Value::String(session.target().circle.clone())),
                    ("database", Value::String(database)),
                    ("nonce", Value::Null),
                    ("ou", Value::Null),
                    ("next_command", Value::String(next_command)),
                ],
            )
        } else {
            error
        }
    })?;
    ensure_receipt_matches_circle(&receipt, &session.target().circle)?;
    let tx_hash = args.tx_hash;
    let mut result = json!({
        "circle": session.target().circle.clone(),
        "tx_hash": tx_hash,
        "result": {},
        "receipt": receipt,
    });
    result = with_explorer(result, &session);
    let success = receipt_result_success(&result);

    if args.json {
        print_json(&json!({
            "ok": success,
            "type": "receipt",
            "schema": "octra-sqlite.cli.v1",
            "database": database_identity(&session),
            "status": if success { "confirmed" } else { "rejected" },
            "tx_hash": result.get("tx_hash").cloned().unwrap_or(Value::Null),
            "tx_url": result.get("tx_url").cloned().unwrap_or(Value::Null),
            "receipt": result.get("receipt").cloned().unwrap_or(Value::Null),
            "result": result,
        }))?;
    } else {
        print_exec_result(&result)?;
    }
    Ok(if success { 0 } else { 1 })
}

pub(super) fn receipt_result_success(result: &Value) -> bool {
    ExecuteResult::from_value(result.clone()).is_ok()
}

pub(super) fn ensure_receipt_matches_circle(receipt: &Value, expected_circle: &str) -> Result<()> {
    let Some(actual_circle) = receipt_circle(receipt) else {
        return Err(Error::with_code_and_details(
            ErrorKind::Receipt,
            "receipt_target_mismatch",
            format!(
                "receipt does not identify its Circle; refusing to confirm against {expected_circle}"
            ),
            [("circle", Value::String(expected_circle.to_string()))],
        )
        .into());
    };
    if actual_circle != expected_circle {
        return Err(Error::with_code_and_details(
            ErrorKind::Receipt,
            "receipt_target_mismatch",
            format!(
                "receipt Circle {actual_circle} does not match selected database Circle {expected_circle}"
            ),
            [
                ("circle", Value::String(expected_circle.to_string())),
                ("receipt_circle", Value::String(actual_circle.to_string())),
            ],
        )
        .into());
    }
    Ok(())
}

fn receipt_circle(receipt: &Value) -> Option<&str> {
    receipt
        .get("contract")
        .or_else(|| receipt.get("circle"))
        .or_else(|| receipt.get("to_"))
        .or_else(|| receipt.get("to"))
        .and_then(Value::as_str)
}

pub(super) fn verify(
    session: &Session,
    expected_hash: Option<&str>,
    write_smoke: bool,
    write_ou: Option<&str>,
    integrity: bool,
    json_mode: bool,
) -> Result<()> {
    if json_mode {
        return verify_json(session, expected_hash, write_smoke, write_ou, integrity);
    }
    print_field("database", &session.target().raw);
    print_field(
        "circle",
        linked_circle(&session.target().network, &session.target().circle),
    );
    let info = program_info(session)?;
    let version = info
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let hash = info
        .get("code_hash")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let bytes = info
        .get("code_bytes")
        .map(value_to_string)
        .unwrap_or_else(|| "unknown".to_string());
    print_field(
        "program",
        format!("version {version}, bytes {bytes}, hash {hash}"),
    );
    if let Some(expected) = expected_hash
        && hash != expected
    {
        if expected == EXPECTED_WASM_SHA256 {
            match personalized_wasm_hash(session) {
                Ok(Some(personalized_hash)) if hash == personalized_hash => {
                    print_field("program", "owner-personalized bundled WASM");
                }
                Ok(Some(personalized_hash)) => bail!(
                    "deployed code hash {hash} does not match expected {expected} or owner-personalized {personalized_hash}"
                ),
                Ok(None) => {
                    bail!("deployed code hash {hash} does not match expected {expected}");
                }
                Err(error) => bail!(
                    "deployed code hash {hash} does not match expected {expected}; personalized check failed: {error:#}"
                ),
            }
        } else {
            bail!("deployed code hash {hash} does not match expected {expected}");
        }
    }
    let storage = view(session, "storage_info", vec![])?;
    print_field(
        "storage",
        format!(
            "{} pages, {} bytes, generation {}",
            storage
                .get("page_count")
                .map(value_to_string)
                .unwrap_or_else(|| "?".to_string()),
            storage
                .get("file_bytes")
                .map(value_to_string)
                .unwrap_or_else(|| "?".to_string()),
            storage
                .get("generation")
                .map(value_to_string)
                .unwrap_or_else(|| "?".to_string())
        ),
    );
    if let Ok(auth) = auth_info(session) {
        if auth.configured {
            print_field(
                "auth",
                format!(
                    "OSW1 owner={}, db_id={}, sequence={}",
                    auth.owner_pubkey.as_deref().unwrap_or("?"),
                    auth.db_id,
                    auth.owner_sequence
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "?".to_string())
                ),
            );
        } else {
            print_field("auth", "unconfigured");
        }
    }
    let sqlite_version = query_typed(session, "select sqlite_version() as sqlite_version;")?;
    print_result(&sqlite_version, OutputMode::Table, true)?;
    let typed_values = query_typed(
        session,
        "select datetime('now') as deterministic_now, 1e3 as real_value, x'4142' as blob_value;",
    )?;
    print_result(&typed_values, OutputMode::Table, true)?;
    let schema = view(session, "schema_typed", vec![])?;
    print_result(&schema, OutputMode::Table, true)?;
    let tables = query_typed(
        session,
        "select name from sqlite_master where type='table' order by name;",
    )?;
    print_result(&tables, OutputMode::Table, true)?;
    if write_smoke {
        let write_ou = resolve_verify_write_ou_arg(write_ou)?;
        let smoke = run_verify_write_smoke(session, &write_ou)?;
        print_exec_result(&smoke.create)?;
        print_result(&smoke.rows, OutputMode::Table, true)?;
        print_exec_result(&smoke.cleanup)?;
    }
    if integrity {
        let integrity = run_verify_integrity(session)?;
        print_field(
            "integrity",
            format!(
                "{result}; checked {} bytes from generation {}",
                integrity.summary.bytes,
                integrity.summary.generation,
                result = integrity.result,
            ),
        );
    }
    Ok(())
}

pub(super) fn verify_json(
    session: &Session,
    expected_hash: Option<&str>,
    write_smoke: bool,
    write_ou: Option<&str>,
    integrity: bool,
) -> Result<()> {
    let info = program_info(session)?;
    let hash = info
        .get("code_hash")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let expected = expected_hash.unwrap_or(EXPECTED_WASM_SHA256);
    let mut program_ok = hash == expected;
    let mut personalized_hash = None;
    if !program_ok && expected == EXPECTED_WASM_SHA256 {
        personalized_hash = personalized_wasm_hash(session)?;
        program_ok = personalized_hash.as_deref() == Some(hash);
    }
    if !program_ok {
        bail!("deployed code hash {hash} does not match expected {expected}");
    }

    let storage = view(session, "storage_info", vec![])?;
    let auth = auth_info(session).ok().map(|auth| {
        json!({
            "configured": auth.configured,
            "owner_pubkey": auth.owner_pubkey,
            "db_id": auth.db_id,
            "owner_sequence": auth.owner_sequence,
        })
    });
    let sqlite_version = query_typed(session, "select sqlite_version() as sqlite_version;")?;
    let schema = view(session, "schema_typed", vec![])?;
    let tables = query_typed(
        session,
        "select name from sqlite_master where type='table' order by name;",
    )?;
    let write_smoke_result = if write_smoke {
        let write_ou = resolve_verify_write_ou_arg(write_ou)?;
        let smoke = run_verify_write_smoke(session, &write_ou)?;
        let mut envelope = write_envelope(session, smoke.create, Some(2));
        if let Some(object) = envelope.as_object_mut() {
            object.insert("rows".to_string(), smoke.rows);
            object.insert(
                "cleanup".to_string(),
                write_envelope(session, smoke.cleanup, Some(1)),
            );
        }
        Some(envelope)
    } else {
        None
    };
    let integrity_result = if integrity {
        let integrity = run_verify_integrity(session)?;
        Some(json!({
            "result": integrity.result,
            "bytes": integrity.summary.bytes,
            "pages": integrity.summary.pages,
            "generation": integrity.summary.generation,
            "sha256": integrity.summary.sha256,
        }))
    } else {
        None
    };
    print_json(&json!({
        "ok": true,
        "type": "verify",
        "schema": "octra-sqlite.cli.v1",
        "database": database_identity(session),
        "program": {
            "info": info,
            "expected_hash": expected,
            "personalized_hash": personalized_hash,
        },
        "storage": storage,
        "auth": auth,
        "sqlite_version": sqlite_version,
        "schema_rows": schema,
        "tables": tables,
        "write_smoke": write_smoke_result,
        "integrity": integrity_result,
    }))
}

pub(super) fn run_verify_write_smoke(
    session: &Session,
    write_ou: &str,
) -> Result<VerifyWriteSmoke> {
    let table = smoke_table_name("verify", session);
    let create = with_explorer(
        exec_sql_with_ou(
            session,
            &format!(
                "create table {table}(first_name text not null, last_name text not null);\n\
                 insert into {table}(first_name,last_name) values ('Ava','North'),('Cora','Moss'),('Drew','Vale');"
            ),
            false,
            write_ou,
        )?,
        session,
    );
    let rows = query_typed(
        session,
        &format!("select first_name,last_name from {table} order by first_name;"),
    );
    let cleanup = exec_sql_with_ou(session, &format!("drop table {table};"), false, write_ou)
        .map(|result| with_explorer(result, session));
    match (rows, cleanup) {
        (Ok(rows), Ok(cleanup)) => Ok(VerifyWriteSmoke {
            create,
            rows,
            cleanup,
        }),
        (Err(query), Ok(_)) => Err(query.into()),
        (Ok(_), Err(cleanup)) => Err(cleanup.into()),
        (Err(query), Err(cleanup)) => Err(anyhow!(
            "write smoke query failed: {query}; cleanup also failed: {cleanup}"
        )),
    }
}

pub(super) fn run_verify_integrity(session: &Session) -> Result<VerifyIntegrity> {
    let path = env::temp_dir().join(format!("{}.sqlite", smoke_table_name("integrity", session)));
    let summary = match backup_database(session, &path) {
        Ok(summary) => summary,
        Err(error) => {
            let _ = fs::remove_file(&path);
            return Err(error);
        }
    };
    let result = run_local_sqlite_integrity(&path);
    let _ = fs::remove_file(&path);
    Ok(VerifyIntegrity {
        summary,
        result: result?,
    })
}

pub(super) fn smoke_table_name(scope: &str, session: &Session) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_be_bytes();
    let digest = h256_raw_frame(
        "octra-sqlite.smoke-table.v1",
        &[
            scope.as_bytes(),
            session.target().circle.as_bytes(),
            session.caller().as_bytes(),
            &std::process::id().to_be_bytes(),
            &nonce,
        ],
    );
    format!("octra_sqlite_{scope}_{}", hex::encode(&digest[..12]))
}
