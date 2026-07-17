use super::*;

pub(super) struct WasmArtifact {
    pub(super) source: String,
    pub(super) bytes: Vec<u8>,
}

pub(super) struct TextArtifact {
    pub(super) source: String,
    pub(super) text: String,
}

pub(super) fn hex_to_32(label: &str, text: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(text).with_context(|| format!("decoding {label} hex"))?;
    if bytes.len() != 32 {
        bail!("{label} must decode to 32 bytes");
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

pub(super) struct CreatedCircle {
    pub(super) circle: String,
    pub(super) owner: String,
    pub(super) code_hash: String,
    pub(super) code_bytes: usize,
    pub(super) auth_patch: AuthPatch,
    pub(super) tx_hash: Option<String>,
    pub(super) confirmation: Option<Value>,
}

#[derive(Clone, Debug)]
pub(super) struct AuthPatch {
    pub(super) owner_pubkey_hex: String,
    pub(super) db_id_hex: String,
    pub(super) owner_pubkey_offset: usize,
    pub(super) db_id_offset: usize,
}

pub(super) fn create_circle(
    session: &Session,
    args: &NewArgs,
    network: &str,
    read_mode: ReadMode,
) -> Result<CreatedCircle> {
    let artifact = resolve_wasm_for_new(args)?;
    let mut wasm = artifact.bytes;
    let auth_patch = patch_wasm_auth_for_owner(&mut wasm, session)?;
    let code_hash = sha256_hex(&wasm);
    let code_b64 = general_purpose::STANDARD.encode(&wasm);
    let payload_json = circle_deploy_payload_json(Some(&code_b64), read_mode)?;
    let nonce = next_nonce(session)?;
    let circle = circle_id_of_deploy(session.caller(), nonce as u64, &payload_json);
    let tx = Tx {
        from: session.caller().to_string(),
        to_: circle.clone(),
        amount: "0".to_string(),
        nonce,
        ou: args.create_ou.clone(),
        timestamp: now_timestamp(),
        op_type: "deploy_circle".to_string(),
        encrypted_data: String::new(),
        message: payload_json,
        signature: String::new(),
        public_key: session.public_key_b64()?.to_string(),
    };
    let result = submit_tx(session, tx, true)?;
    let tx_hash = result
        .get("tx_hash")
        .and_then(Value::as_str)
        .map(str::to_string);
    let confirmation = if args.no_wait {
        None
    } else if let Some(hash) = tx_hash.as_deref() {
        Some(wait_for_transaction(session, hash)?)
    } else {
        None
    };
    let circle_session = session.with_database_target(Target {
        raw: format!("oct://{}/{}", network, circle),
        network: network.to_string(),
        circle: circle.clone(),
        rpc: session.rpc().to_string(),
        read_mode,
    });
    if !args.no_wait {
        wait_for_program_info(&circle_session, &code_hash)?;
    }
    Ok(CreatedCircle {
        circle,
        owner: session.caller().to_string(),
        code_hash,
        code_bytes: wasm.len(),
        auth_patch,
        tx_hash,
        confirmation,
    })
}

pub(super) fn with_explorer(mut result: Value, session: &Session) -> Value {
    let Some(object) = result.as_object_mut() else {
        return result;
    };
    if let Some(url) = explorer_circle_url(&session.target().network, &session.target().circle) {
        object.insert("circle_url".to_string(), Value::String(url));
    }
    if let Some(tx_hash) = object
        .get("tx_hash")
        .and_then(Value::as_str)
        .map(str::to_string)
        && let Some(url) = explorer_tx_url(&session.target().network, &tx_hash)
    {
        object.insert("tx_url".to_string(), Value::String(url));
    }
    result
}

pub(super) fn linked_circle(network: &str, circle: &str) -> String {
    match explorer_circle_url(network, circle) {
        Some(url) => hyperlink(circle, url),
        None => circle.to_string(),
    }
}

pub(super) fn linked_tx(network: &str, hash: &str) -> String {
    match explorer_tx_url(network, hash) {
        Some(url) => hyperlink(hash, url),
        None => hash.to_string(),
    }
}

pub(super) fn explorer_base_url(network: &str) -> Option<String> {
    load_config()
        .ok()?
        .explorer_for_network(network)
        .map(|url| url.trim_end_matches('/').to_string())
}

pub(super) fn explorer_tx_url(network: &str, hash: &str) -> Option<String> {
    Some(format!(
        "{}/tx.html?hash={hash}",
        explorer_base_url(network)?
    ))
}

pub(super) fn explorer_circle_url(network: &str, circle: &str) -> Option<String> {
    Some(format!(
        "{}/address.html?addr={circle}",
        explorer_base_url(network)?
    ))
}

pub(super) fn patch_wasm_auth_for_owner(wasm: &mut [u8], session: &Session) -> Result<AuthPatch> {
    let owner_pubkey = session.intent_public_key()?;
    let db_id = derive_db_id(session, &owner_pubkey);
    patch_wasm_auth_bytes(wasm, &owner_pubkey, &db_id)
}

pub(super) fn patch_wasm_auth_bytes(
    wasm: &mut [u8],
    owner_pubkey: &[u8; 32],
    db_id: &[u8; 32],
) -> Result<AuthPatch> {
    let owner_pubkey_offset =
        replace_wasm_placeholder(wasm, OWNER_PUBKEY_PLACEHOLDER, owner_pubkey)
            .context("patching owner public key into Circle WASM")?;
    let db_id_offset = replace_wasm_placeholder(wasm, DB_ID_PLACEHOLDER, db_id)
        .context("patching database id into Circle WASM")?;
    Ok(AuthPatch {
        owner_pubkey_hex: hex::encode(owner_pubkey),
        db_id_hex: hex::encode(db_id),
        owner_pubkey_offset,
        db_id_offset,
    })
}

pub(super) fn patch_wasm_auth_from_info(wasm: &mut [u8], auth: &AuthInfo) -> Result<AuthPatch> {
    if !auth.configured {
        return Err(auth_info_error("auth_info reports unconfigured OSW1 auth"));
    }
    let owner_pubkey = hex_to_32(
        "owner_pubkey",
        auth.owner_pubkey
            .as_deref()
            .ok_or_else(|| auth_info_error("auth_info missing owner_pubkey"))?,
    )?;
    let db_id = hex_to_32("db_id", &auth.db_id)?;
    patch_wasm_auth_bytes(wasm, &owner_pubkey, &db_id)
}

pub(super) fn replace_wasm_placeholder(
    wasm: &mut [u8],
    placeholder: &[u8],
    replacement: &[u8],
) -> Result<usize> {
    if placeholder.len() != replacement.len() {
        bail!("placeholder and replacement lengths differ");
    }
    let mut found = None;
    let mut count = 0usize;
    for (index, window) in wasm.windows(placeholder.len()).enumerate() {
        if window == placeholder {
            found = Some(index);
            count += 1;
        }
    }
    match (found, count) {
        (Some(index), 1) => {
            wasm[index..index + replacement.len()].copy_from_slice(replacement);
            Ok(index)
        }
        (_, 0) => bail!("auth placeholder not found; rebuild the bundled WASM from this checkout"),
        _ => bail!("auth placeholder appeared {count} times; refusing ambiguous patch"),
    }
}

pub(super) fn derive_db_id(session: &Session, owner_pubkey: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"octra-sqlite.db-id.v1");
    hasher.update(session.caller().as_bytes());
    hasher.update(owner_pubkey);
    hasher.update(now_timestamp().to_string().as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub(super) fn resolve_wasm_for_new(args: &NewArgs) -> Result<WasmArtifact> {
    resolve_wasm_artifact(args.build, args.wasm.as_deref())
}

pub(super) fn resolve_wasm_artifact(build: bool, wasm: Option<&Path>) -> Result<WasmArtifact> {
    if build {
        build_wasm_from_checkout()?;
        if let Some(path) = find_project_file(DEFAULT_WASM_REL) {
            return read_wasm_artifact(path);
        }
        bail!("could not find built WASM artifact at {DEFAULT_WASM_REL}");
    }
    if let Some(path) = wasm {
        return read_wasm_artifact(require_file(path.to_path_buf(), "custom WASM")?);
    }
    resolve_bundled_wasm_artifact()
}

pub(super) fn resolve_bundled_wasm_artifact() -> Result<WasmArtifact> {
    let bytes = EMBEDDED_WASM_BYTES.to_vec();
    let hash = sha256_hex(&bytes);
    if bytes.len() != EXPECTED_WASM_BYTES || hash != EXPECTED_WASM_SHA256 {
        bail!(
            "embedded release WASM failed integrity check: {hash}, {} bytes; expected {}, {} bytes",
            bytes.len(),
            EXPECTED_WASM_SHA256,
            EXPECTED_WASM_BYTES
        );
    }
    Ok(WasmArtifact {
        source: format!("embedded:{DEFAULT_WASM_REL}"),
        bytes,
    })
}

pub(super) fn read_wasm_artifact(path: PathBuf) -> Result<WasmArtifact> {
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(WasmArtifact {
        source: path.display().to_string(),
        bytes,
    })
}

pub(super) fn resolve_release_manifest() -> Result<TextArtifact> {
    if let Ok(path) = env::var("OCTRA_SQLITE_MANIFEST") {
        return read_text_artifact(require_file(PathBuf::from(path), "OCTRA_SQLITE_MANIFEST")?);
    }
    Ok(TextArtifact {
        source: format!("embedded:{RELEASE_MANIFEST_REL}"),
        text: EMBEDDED_RELEASE_MANIFEST.to_string(),
    })
}

pub(super) fn read_text_artifact(path: PathBuf) -> Result<TextArtifact> {
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(TextArtifact {
        source: path.display().to_string(),
        text,
    })
}

pub(super) fn require_file(path: PathBuf, label: &str) -> Result<PathBuf> {
    if path.is_file() {
        Ok(path)
    } else {
        bail!(
            "{label} does not exist or is not a file: {}",
            path.display()
        )
    }
}

pub(super) fn build_wasm_from_checkout() -> Result<()> {
    let Some(script) = find_project_file(BUILD_WASM_SCRIPT_REL) else {
        bail!("could not find {BUILD_WASM_SCRIPT_REL}")
    };
    let root = script
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow!("could not determine project root for {}", script.display()))?;
    let status = ProcessCommand::new("bash")
        .arg(&script)
        .current_dir(root)
        .status()
        .with_context(|| format!("running {}", script.display()))?;
    if !status.success() {
        bail!("{} failed", script.display());
    }
    Ok(())
}

pub(super) fn find_project_file(relative: &str) -> Option<PathBuf> {
    for root in project_roots() {
        let path = root.join(relative);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

pub(super) fn project_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(root) = env::var("OCTRA_SQLITE_ROOT") {
        roots.push(PathBuf::from(root));
    }
    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd);
    }
    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent()
    {
        roots.push(dir.to_path_buf());
    }
    let mut unique = Vec::new();
    for root in roots {
        if !unique.iter().any(|existing: &PathBuf| existing == &root) {
            unique.push(root);
        }
    }
    unique
}

pub(super) fn circle_deploy_payload_json(
    code_b64: Option<&str>,
    read_mode: ReadMode,
) -> Result<String> {
    let code = match code_b64 {
        Some(value) => serde_json::to_string(value)?,
        None => "null".to_string(),
    };
    let (privacy_class, browser_mode, resource_mode) = deploy_tuple(read_mode);
    Ok(format!(
        "{{\"runtime\":\"wasm_v1\",\"privacy_class\":\"{privacy_class}\",\"browser_mode\":\"{browser_mode}\",\"resource_mode\":\"{resource_mode}\",\"code_b64\":{},\"policy_hash\":null,\"members_root\":null,\"export_policy\":null,\"limits\":{{\"max_stable_bytes\":\"33554432\",\"max_assets_bytes\":\"33554432\",\"max_inline_value\":\"65536\",\"max_wasm_bytes\":\"33554432\"}}}}",
        code,
    ))
}

pub(super) fn deploy_tuple(read_mode: ReadMode) -> (&'static str, &'static str, &'static str) {
    match read_mode {
        ReadMode::Public => ("public", "gateway_allowed", "public_resources"),
        ReadMode::Auto | ReadMode::Sealed => ("sealed", "native_sealed", "sealed_read"),
    }
}

pub(super) fn circle_id_of_deploy(deployer: &str, nonce: u64, payload_json: &str) -> String {
    let payload_hash = h256_hex_frame("octra:circle_deploy_payload:v1", &[payload_json.as_bytes()]);
    let nonce_bytes = nonce.to_be_bytes();
    let seed = h256_raw_frame(
        "octra:circle_deploy_id:v1",
        &[deployer.as_bytes(), &nonce_bytes, payload_hash.as_bytes()],
    );
    let base58 = base58::encode(&seed);
    let part = if base58.len() >= 44 {
        base58[..44].to_string()
    } else if base58.is_empty() {
        "1".repeat(44)
    } else {
        base58
            .repeat((44usize).div_ceil(base58.len()))
            .chars()
            .take(44)
            .collect()
    };
    format!("oct{part}")
}

pub(super) fn h256_raw_frame(tag: &str, parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(tag.as_bytes());
    hasher.update([0]);
    for part in parts {
        hasher.update((part.len() as u32).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().into()
}

pub(super) fn h256_hex_frame(tag: &str, parts: &[&[u8]]) -> String {
    hex::encode(h256_raw_frame(tag, parts))
}

pub(super) fn cmd_deploy(args: DeployArgs) -> Result<()> {
    let circle = args
        .circle
        .clone()
        .ok_or_else(|| anyhow!("deploy requires --circle CIRCLE_ID or oct://NETWORK/CIRCLE_ID"))?;
    if args.bootstrap_owner {
        if !circle.starts_with("oct://") {
            bail!("--bootstrap-owner requires --circle oct://NETWORK/CIRCLE_ID");
        }
        if args.wasm.is_some() {
            bail!("--bootstrap-owner uses the bundled Circle WASM; omit --wasm");
        }
        if args.build {
            bail!("--bootstrap-owner uses the bundled Circle WASM; omit --build");
        }
        if args.no_wait {
            bail!("--bootstrap-owner requires receipt confirmation; omit --no-wait");
        }
    }
    let target_args = TargetArgs {
        target: Some(circle.clone()),
        wallet: args.wallet.clone(),
        rpc: args.rpc.clone(),
        caller: args.caller.clone(),
        private_key_b64: args.private_key_b64.clone(),
        public_key_b64: args.public_key_b64.clone(),
    };
    let session = build_session(&target_args)?;
    let artifact = if args.bootstrap_owner {
        resolve_bundled_wasm_artifact()?
    } else {
        resolve_wasm_artifact(args.build, args.wasm.as_deref())?
    };
    let wasm_source = artifact.source.clone();
    let mut wasm = artifact.bytes;
    if args.bootstrap_owner && args.allow_unconfigured {
        bail!("--bootstrap-owner and --allow-unconfigured are mutually exclusive");
    }
    let auth_patch = if args.bootstrap_owner {
        let info = program_info(&session)
            .context("reading Circle program info before owner bootstrap deploy")?;
        match program_owner(&info) {
            Some(owner) if owner == session.caller() => {}
            Some(owner) => {
                return Err(auth_error(format!(
                    "Circle owner is {owner}; current wallet {} cannot bootstrap owner-personalized WASM",
                    session.caller()
                )));
            }
            None => {
                return Err(auth_error(
                    "Circle program info did not expose an owner; refusing bootstrap deploy",
                ));
            }
        }
        Some(
            patch_wasm_auth_for_owner(&mut wasm, &session)
                .context("patching owner bootstrap auth into Circle WASM")?,
        )
    } else {
        match auth_info(&session) {
        Ok(auth) if auth.configured => Some(patch_wasm_auth_from_info(&mut wasm, &auth).with_context(|| {
            "preserving existing OSW1 personalization; pass --allow-unconfigured to deploy raw WASM"
        })?),
        Ok(_) if args.allow_unconfigured => None,
        Ok(_) => bail!(
            "database Circle is not OSW1-personalized; refusing to deploy raw unsigned-write WASM without --allow-unconfigured"
        ),
        Err(error) if args.allow_unconfigured => {
            eprintln!("warning: auth_info unavailable; deploying unconfigured WASM because --allow-unconfigured was passed: {error:#}");
            None
        }
        Err(error) => bail!(
            "could not read database auth_info; refusing to deploy because it could remove owner-write protection: {error:#}. Pass --allow-unconfigured to deploy raw WASM."
        ),
        }
    };
    let code_hash = sha256_hex(&wasm);
    let message = serde_json::to_string(&json!({
        "code_b64": general_purpose::STANDARD.encode(&wasm),
    }))?;
    let tx = Tx {
        from: session.caller().to_string(),
        to_: session.target().circle.clone(),
        amount: "0".to_string(),
        nonce: next_nonce(&session)?,
        ou: args.ou,
        timestamp: now_timestamp(),
        op_type: "circle_program_update".to_string(),
        encrypted_data: String::new(),
        message,
        signature: String::new(),
        public_key: session.public_key_b64()?.to_string(),
    };
    let result = submit_tx(&session, tx, true)?;
    let tx_hash = result
        .get("tx_hash")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut out = Map::new();
    out.insert(
        "circle".to_string(),
        Value::String(session.target().circle.clone()),
    );
    out.insert("wasm".to_string(), Value::String(wasm_source));
    out.insert("code_bytes".to_string(), Value::Number(wasm.len().into()));
    out.insert("code_hash".to_string(), Value::String(code_hash.clone()));
    out.insert(
        "bootstrap_owner".to_string(),
        Value::Bool(args.bootstrap_owner),
    );
    if let Some(patch) = auth_patch.as_ref() {
        out.insert(
            "auth_patch".to_string(),
            json!({
                "owner_pubkey": patch.owner_pubkey_hex,
                "db_id": patch.db_id_hex,
                "owner_pubkey_offset": patch.owner_pubkey_offset,
                "db_id_offset": patch.db_id_offset,
            }),
        );
    }
    out.insert("program_update".to_string(), result);
    if let Some(hash) = tx_hash.clone() {
        out.insert("tx_hash".to_string(), Value::String(hash.clone()));
        if !args.no_wait {
            let confirmation = wait_for_transaction(&session, &hash)?;
            out.insert(
                "confirmation".to_string(),
                redact_code_payload(confirmation),
            );
        }
    }
    if !args.no_wait {
        let info = wait_for_program_info(&session, &code_hash)?;
        out.insert("program_info".to_string(), info);
    }
    if args.bootstrap_owner {
        let patch = auth_patch
            .as_ref()
            .ok_or_else(|| anyhow!("bootstrap-owner deploy missing auth patch"))?;
        let saved = save_bootstrap_owner_metadata(
            &session,
            patch,
            &code_hash,
            wasm.len(),
            tx_hash.clone(),
        )?;
        out.insert("metadata_saved".to_string(), json!(saved));
    }
    print_json(&Value::Object(out))
}

pub(super) fn redact_code_payload(value: Value) -> Value {
    let Value::Object(mut map) = value else {
        return value;
    };
    if map
        .get("message")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("\"code_b64\""))
    {
        map.insert(
            "message".to_string(),
            Value::String("{\"code_b64\":\"<redacted>\"}".to_string()),
        );
    }
    Value::Object(map)
}

pub(super) fn save_bootstrap_owner_metadata(
    session: &Session,
    patch: &AuthPatch,
    code_hash: &str,
    code_bytes: usize,
    tx_hash: Option<String>,
) -> Result<Vec<String>> {
    save_database_metadata(
        session,
        &patch.owner_pubkey_hex,
        &patch.db_id_hex,
        code_hash,
        code_bytes,
        tx_hash,
    )
}

pub(super) fn save_database_metadata(
    session: &Session,
    owner_pubkey: &str,
    db_id: &str,
    code_hash: &str,
    code_bytes: usize,
    tx_hash: Option<String>,
) -> Result<Vec<String>> {
    let mut config = load_config()?;
    let uri = canonical_database_uri(session.target());
    let mut keys = config
        .databases
        .iter()
        .filter_map(|(name, database)| {
            resolve_target(database, &config)
                .ok()
                .filter(|target| {
                    target.network == session.target().network
                        && target.circle == session.target().circle
                })
                .map(|_| name.clone())
        })
        .collect::<Vec<_>>();
    if keys.is_empty() {
        keys.push(uri.clone());
    }
    let create_tx = config
        .database_metadata
        .values()
        .find(|metadata| {
            metadata.uri == uri
                || (metadata.network == session.target().network
                    && metadata.circle == session.target().circle)
        })
        .and_then(|metadata| metadata.create_tx.clone());
    for key in &keys {
        config.database_metadata.insert(
            key.clone(),
            DatabaseMetadata {
                uri: uri.clone(),
                network: session.target().network.clone(),
                circle: session.target().circle.clone(),
                read_mode: session.target().read_mode,
                privacy_class: deploy_tuple(session.target().read_mode).0.to_string(),
                browser_mode: deploy_tuple(session.target().read_mode).1.to_string(),
                resource_mode: deploy_tuple(session.target().read_mode).2.to_string(),
                owner: session.caller().to_string(),
                owner_pubkey: owner_pubkey.to_string(),
                db_id: db_id.to_string(),
                code_hash: code_hash.to_string(),
                code_bytes,
                create_tx: create_tx.clone(),
                program_update_tx: tx_hash.clone(),
            },
        );
    }
    write_config(&config)?;
    Ok(keys)
}

pub(super) fn wait_for_program_info(session: &Session, expected_hash: &str) -> Result<Value> {
    for _ in 0..30 {
        if let Ok(info) = program_info(session) {
            let hash = info
                .get("code_hash")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if hash == expected_hash {
                return Ok(info);
            }
            bail!("deployed code hash {hash} does not match expected {expected_hash}");
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    bail!("timed out waiting for deployed program info")
}
