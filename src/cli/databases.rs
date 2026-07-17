use super::*;

pub(super) fn cmd_database(command: DatabaseCommand) -> Result<()> {
    let mut config = load_config()?;
    match command {
        DatabaseCommand::List { json } => print_database_list(&config, json)?,
        DatabaseCommand::Info { database, json } => {
            print_database_info(&config, database.as_deref(), json)?
        }
        DatabaseCommand::Set { name, database } => {
            parse_target_uri(&database, &config)?;
            config.databases.insert(name.clone(), database.clone());
            if config.default_database.is_none() {
                config.default_database = Some(name.clone());
            }
            write_config(&config)?;
            print_field("database", format!("{name} -> {database}"));
            print_field("open", format!("octra-sqlite {name}"));
        }
        DatabaseCommand::Default { name } => {
            if !config.databases.contains_key(&name) {
                bail!("unknown database {name}; run octra-sqlite database list");
            }
            config.default_database = Some(name.clone());
            write_config(&config)?;
            print_field("default database", name);
            print_field("open", "octra-sqlite");
        }
        DatabaseCommand::Remove { name } => {
            config.databases.remove(&name);
            config.database_metadata.remove(&name);
            if config.default_database.as_deref() == Some(&name) {
                config.default_database = None;
            }
            write_config(&config)?;
            print_field("removed", name);
        }
    }
    Ok(())
}

pub(super) fn print_database_list(config: &Config, json_mode: bool) -> Result<()> {
    if json_mode {
        let databases = config
            .databases
            .iter()
            .map(|(name, uri)| {
                let read_mode = resolve_target(name, config)
                    .map(|target| target.read_mode.as_str().to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                json!({
                    "name": name,
                    "uri": uri,
                    "read_mode": read_mode,
                    "default": config.default_database.as_deref() == Some(name),
                })
            })
            .collect::<Vec<_>>();
        return print_json(&json!({
            "ok": true,
            "type": "database_list",
            "schema": "octra-sqlite.cli.v1",
            "default_database": config.default_database,
            "databases": databases,
        }));
    }
    if config.databases.is_empty() {
        println!("{}", dim("no databases"));
        print_field("create", CREATE_DATABASE_COMMAND);
        return Ok(());
    }
    println!("{}  name  read_mode  uri", dim("default"));
    println!("{}", dim("-------  ----  ---------  ---"));
    for (name, database) in &config.databases {
        let default_mark = if config.default_database.as_deref() == Some(name) {
            "*"
        } else {
            ""
        };
        let read_mode = resolve_target(name, config)
            .map(|target| target.read_mode.as_str().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        println!("{default_mark:<7}  {name}  {read_mode:<9}  {database}");
    }
    Ok(())
}

pub(super) fn print_database_info(
    config: &Config,
    database: Option<&str>,
    json_mode: bool,
) -> Result<()> {
    let requested = database
        .map(str::to_string)
        .or_else(|| config.default_database.clone())
        .ok_or_else(|| anyhow!("no database supplied and no default database is configured"))?;
    let saved_uri = config.databases.get(&requested);
    let metadata = config.database_metadata.get(&requested);
    let target = resolve_target(&requested, config)?;
    if json_mode {
        return print_json(&json!({
            "ok": true,
            "type": "database_info",
            "schema": "octra-sqlite.cli.v1",
            "name": if saved_uri.is_some() { Some(requested.as_str()) } else { None },
            "default": config.default_database.as_deref() == Some(requested.as_str()),
            "database": {
                "uri": canonical_database_uri(&target),
                "raw": target.raw,
                "network": target.network,
                "circle": target.circle,
                "rpc": target.rpc,
                "read_mode": target.read_mode.as_str(),
            },
            "metadata": metadata,
        }));
    }
    print_field(
        "name",
        if saved_uri.is_some() {
            requested.as_str()
        } else {
            "(not saved)"
        },
    );
    print_field(
        "default",
        (config.default_database.as_deref() == Some(requested.as_str())).to_string(),
    );
    print_field("uri", &target.raw);
    print_field("read_mode", target.read_mode.as_str());
    print_field("network", &target.network);
    print_field("circle", linked_circle(&target.network, &target.circle));
    print_field(
        "rpc",
        if target.rpc.is_empty() {
            "(not configured)"
        } else {
            target.rpc.as_str()
        },
    );
    if let Some(explorer) = config.explorer_for_network(&target.network) {
        print_field("explorer", explorer);
    }
    if let Some(metadata) = metadata {
        print_field("owner", &metadata.owner);
        print_field("code hash", &metadata.code_hash);
    }
    print_field("open", format!("octra-sqlite {}", requested));
    print_field("status", format!("octra-sqlite status {}", requested));
    Ok(())
}
