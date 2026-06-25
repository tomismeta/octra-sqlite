use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use clap::{Args, Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey};
mod osr1;
mod output;
use osr1::{decode_typed_result, TYPED_PREFIX};
use output::{
    format_exec_result, format_json, format_result, print_exec_result, print_json, print_result,
    value_to_string, write_text, OutputMode,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_RPC: &str = "http://165.227.225.79:8080/rpc";
const DEFAULT_CIRCLE: &str = "oct236kMS6wDL9r3S5UnwgFJoS79V4kPUpgPVcQNcdnfsPR";
const DEFAULT_CALLER: &str = "octCpJ1SJNi7NBNEjo9DnMfhy4fH3HGDrXN7JL1UhoGYgCB";
const DEFAULT_NETWORK: &str = "devnet";
const DEFAULT_WASM_REL: &str = "circle/wasm/octra_sqlite_circle.wasm";
const BUILD_WASM_SCRIPT_REL: &str = "scripts/build-wasm.sh";
const RELEASE_MANIFEST_REL: &str = "release/octra-sqlite-0.1.0.json";
const OWNER_PUBKEY_PLACEHOLDER: &[u8; 32] = b"OSQL_OWNER_PUBKEY_V1_PLACEHOLDER";
const DB_ID_PLACEHOLDER: &[u8; 32] = b"OSQL_DATABASE_ID_V1_PLACEHOLDER0";
const OWNER_WRITE_INTENT_DOMAIN: &[u8] = b"octra-sqlite.osw1.v1\0";
const EXPECTED_WASM_SHA256: &str =
    "0e28ecc233306fd59539a22209be633fa7e6ca7410c84ce7c940abfcfb372e7a";
const EXPECTED_WASM_BYTES: usize = 607_496;

#[derive(Parser)]
#[command(name = "octra-sqlite", version)]
#[command(about = "SQLite CLI for Octra-backed databases")]
#[command(after_long_help = "\
Examples:
  octra-sqlite setup
  octra-sqlite status
  octra-sqlite config
  octra-sqlite new organization < examples/organization-person.sql
  octra-sqlite organization \".tables\"
  octra-sqlite organization \"select rowid, first_name, last_name from person;\"
  octra-sqlite database list
  octra-sqlite database info organization
")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive setup wizard for wallet, RPC, and optional sample database.
    Setup(SetupArgs),
    /// Configure wallet, RPC, network, and optional default database.
    Init(InitArgs),
    /// Create a sample SQLite database in one command.
    Quickstart(QuickstartArgs),
    /// Create a new SQLite database on Octra and optionally initialize it with SQL.
    New(NewArgs),
    /// Manage saved database names.
    #[command(alias = "db")]
    Database {
        #[command(subcommand)]
        command: DatabaseCommand,
    },
    /// Legacy spelling for database.
    #[command(hide = true)]
    Alias {
        #[command(subcommand)]
        command: DatabaseCommand,
    },
    /// Open a SQLite shell or run SQL against a database.
    Open(OpenArgs),
    /// Run read-only SQL.
    #[command(hide = true)]
    Query(SqlArgs),
    /// Run state-changing SQL and wait for a receipt.
    #[command(hide = true)]
    Exec(SqlArgs),
    /// Show tables from sqlite_master.
    #[command(hide = true)]
    Tables(TargetArgs),
    /// Show SQLite schema.
    #[command(hide = true)]
    Schema(TargetArgs),
    /// Show SQLite page storage info.
    #[command(hide = true)]
    Storage(TargetArgs),
    /// Show Octra Circle program metadata.
    #[command(hide = true)]
    Circle(TargetArgs),
    /// Prove live database metadata, storage, SQLite version, and tables.
    #[command(hide = true)]
    Proof(TargetArgs),
    /// Verify deployed database code, storage, OSR1 typed queries, schema, and optionally a write.
    Verify(VerifyArgs),
    /// Show local config, wallet, bundled WASM, and live database health.
    Status(DoctorArgs),
    /// Legacy spelling for status.
    #[command(hide = true)]
    Doctor(DoctorArgs),
    /// Show local wallet, RPC, network, and database configuration.
    Config(ConfigArgs),
    /// Deploy/update a Circle program through native signed RPC.
    Deploy(DeployArgs),
    /// Print installation instructions for the Rust CLI.
    Install,
}

#[derive(Subcommand)]
#[command(after_long_help = "\
Examples:
  octra-sqlite database list
  octra-sqlite database info organization
  octra-sqlite database use organization
  octra-sqlite database set organization oct://devnet/oct...
")]
enum DatabaseCommand {
    /// List saved database names.
    List,
    /// Show the URI, network, Circle id, and RPC for a database.
    Info {
        /// Database name, Circle id, or oct:// database URI. Defaults to the current database.
        #[arg(value_name = "DATABASE")]
        database: Option<String>,
    },
    /// Save a database name for an Octra database URI.
    Set {
        name: String,
        #[arg(value_name = "DATABASE_URI")]
        database: String,
    },
    /// Set the default database opened when no database is supplied.
    Use { name: String },
    /// Remove a saved database name.
    #[command(alias = "rm")]
    Remove { name: String },
}

#[derive(Args, Clone)]
struct TargetArgs {
    /// Database name, Circle id, or oct:// database URI.
    #[arg(value_name = "DATABASE")]
    target: Option<String>,
    /// Wallet JSON path. Auto-detects ./wallet.json when omitted.
    #[arg(long)]
    wallet: Option<PathBuf>,
    /// Octra RPC URL.
    #[arg(long)]
    rpc: Option<String>,
    /// Caller wallet address override.
    #[arg(long)]
    caller: Option<String>,
    /// Private key override, base64 or hex.
    #[arg(long)]
    private_key_b64: Option<String>,
    /// Public key override, base64.
    #[arg(long)]
    public_key_b64: Option<String>,
}

#[derive(Args)]
struct InitArgs {
    /// Wallet JSON path. Auto-detects ./wallet.json when omitted.
    #[arg(long)]
    wallet: Option<PathBuf>,
    /// Octra RPC URL.
    #[arg(long, default_value = DEFAULT_RPC)]
    rpc: String,
    /// Octra network name.
    #[arg(long, default_value = DEFAULT_NETWORK)]
    network: String,
    /// Default database name, Circle id, or oct:// database URI.
    #[arg(long, alias = "target")]
    database: Option<String>,
}

#[derive(Args, Clone)]
struct SetupArgs {
    /// Wallet JSON path. Auto-detects ./wallet.json when omitted.
    #[arg(long)]
    wallet: Option<PathBuf>,
    /// Octra RPC URL.
    #[arg(long)]
    rpc: Option<String>,
    /// Octra network name.
    #[arg(long)]
    network: Option<String>,
    /// Default database name, Circle id, or oct:// database URI.
    #[arg(long, alias = "target")]
    database: Option<String>,
    /// Use discovered values and defaults without prompting.
    #[arg(long)]
    yes: bool,
}

#[derive(Args)]
struct OpenArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// Print raw JSON instead of table or compact receipt output.
    #[arg(long)]
    json: bool,
    /// SQL to run directly instead of opening the shell.
    sql: Vec<String>,
}

#[derive(Args)]
struct SqlArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long)]
    sql: Option<String>,
    #[arg(long)]
    no_wait: bool,
}

#[derive(Args)]
struct NewArgs {
    /// Local database name for the new database.
    name: String,
    /// Rebuild the bundled WASM before deploying.
    #[arg(long)]
    build: bool,
    /// Custom WASM program to deploy into the new Circle.
    #[arg(long)]
    wasm: Option<PathBuf>,
    /// OU budget for Circle creation.
    #[arg(long, default_value = "200000")]
    create_ou: String,
    /// Octra RPC URL.
    #[arg(long, default_value = DEFAULT_RPC)]
    rpc: String,
    /// Octra network name.
    #[arg(long, default_value = DEFAULT_NETWORK)]
    network: String,
    /// Do not wait for Circle creation confirmation or initializer SQL receipts.
    #[arg(long)]
    no_wait: bool,
    /// Do not save a local database name.
    #[arg(long = "no-name", alias = "no-alias")]
    no_name: bool,
    /// Make the new database the default database.
    #[arg(long)]
    default: bool,
    /// SQL to run after creating the database.
    #[arg(long)]
    sql: Option<String>,
    /// SQL file to run after creating the database.
    #[arg(long)]
    read: Option<PathBuf>,
    /// Initialize with a built-in sample schema and rows.
    #[arg(long, value_name = "NAME")]
    sample: Option<String>,
    /// Wallet JSON path. Auto-detects ./wallet.json when omitted.
    #[arg(long)]
    wallet: Option<PathBuf>,
    /// Caller wallet address override.
    #[arg(long)]
    caller: Option<String>,
    /// Private key override, base64 or hex.
    #[arg(long)]
    private_key_b64: Option<String>,
    /// Public key override, base64.
    #[arg(long)]
    public_key_b64: Option<String>,
    /// SQL to run after creating the database, sqlite3-style.
    #[arg(value_name = "SQL")]
    sql_args: Vec<String>,
}

#[derive(Args, Clone)]
struct QuickstartArgs {
    /// Local database name for the sample database.
    name: String,
    /// Built-in sample to install.
    #[arg(long, default_value = "people")]
    sample: String,
    /// Rebuild the bundled WASM before deploying.
    #[arg(long)]
    build: bool,
    /// Custom WASM program to deploy into the new Circle.
    #[arg(long)]
    wasm: Option<PathBuf>,
    /// OU budget for Circle creation.
    #[arg(long, default_value = "200000")]
    create_ou: String,
    /// Octra RPC URL.
    #[arg(long)]
    rpc: Option<String>,
    /// Octra network name.
    #[arg(long)]
    network: Option<String>,
    /// Do not wait for Circle creation confirmation or initializer SQL receipts.
    #[arg(long)]
    no_wait: bool,
    /// Do not make the new database the default database.
    #[arg(long)]
    no_default: bool,
    /// Wallet JSON path. Auto-detects ./wallet.json when omitted.
    #[arg(long)]
    wallet: Option<PathBuf>,
    /// Caller wallet address override.
    #[arg(long)]
    caller: Option<String>,
    /// Private key override, base64 or hex.
    #[arg(long)]
    private_key_b64: Option<String>,
    /// Public key override, base64.
    #[arg(long)]
    public_key_b64: Option<String>,
}

#[derive(Args)]
struct DeployArgs {
    /// Rebuild the bundled WASM before deploying.
    #[arg(long)]
    build: bool,
    /// Circle id to update.
    #[arg(long)]
    circle: Option<String>,
    /// Custom WASM program to deploy.
    #[arg(long)]
    wasm: Option<PathBuf>,
    /// OU budget for Circle program update.
    #[arg(long, default_value = "200000")]
    ou: String,
    /// Octra RPC URL.
    #[arg(long, default_value = DEFAULT_RPC)]
    rpc: String,
    /// Do not wait for update confirmation.
    #[arg(long)]
    no_wait: bool,
    /// Allow deploying unpersonalized WASM that has unsigned writes.
    #[arg(long)]
    allow_unconfigured: bool,
    /// Wallet JSON path. Auto-detects ./wallet.json when omitted.
    #[arg(long)]
    wallet: Option<PathBuf>,
    /// Caller wallet address override.
    #[arg(long)]
    caller: Option<String>,
    /// Private key override, base64 or hex.
    #[arg(long)]
    private_key_b64: Option<String>,
    /// Public key override, base64.
    #[arg(long)]
    public_key_b64: Option<String>,
}

#[derive(Args)]
struct VerifyArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// Expected deployed code hash. Defaults to the bundled release artifact hash.
    #[arg(long)]
    expected_hash: Option<String>,
    /// Run a live write/read smoke test against the database.
    #[arg(long)]
    write_smoke: bool,
}

#[derive(Args)]
struct DoctorArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// Expected deployed code hash. Defaults to the bundled release artifact hash.
    #[arg(long)]
    expected_hash: Option<String>,
    /// Do not call Octra RPC; only inspect local checkout/config/wallet.
    #[arg(long)]
    skip_network: bool,
}

#[derive(Args)]
struct ConfigArgs {
    /// Print raw JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Config {
    wallet: Option<String>,
    rpc: Option<String>,
    network: Option<String>,
    #[serde(default, alias = "default_target")]
    default_database: Option<String>,
    #[serde(default, alias = "aliases")]
    databases: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
struct WalletFile {
    addr: Option<String>,
    address: Option<String>,
    priv_: Option<String>,
    #[serde(rename = "priv")]
    priv_field: Option<String>,
    private_key: Option<String>,
    private_key_b64: Option<String>,
    pub_: Option<String>,
    #[serde(rename = "pub")]
    pub_field: Option<String>,
    public_key: Option<String>,
    public_key_b64: Option<String>,
    rpc: Option<String>,
}

#[derive(Debug, Clone)]
struct Target {
    raw: String,
    network: String,
    circle: String,
    rpc: String,
}

#[derive(Clone)]
struct Session {
    target: Target,
    wallet_path: Option<PathBuf>,
    rpc: String,
    caller: String,
    private_key_text: String,
    public_key_b64: String,
}

struct ShellState {
    session: Session,
    mode: OutputMode,
    headers: bool,
    timer: bool,
    output: Option<PathBuf>,
}

fn main() {
    if let Err(error) = run_cli() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run_cli() -> Result<()> {
    let args = normalize_args(env::args().collect());
    let bare_init = args.len() == 2 && args[1] == "init";
    let cli = Cli::parse_from(args);
    match cli.command {
        Commands::Setup(args) => cmd_setup(args),
        Commands::Init(args) => {
            if bare_init && io::stdin().is_terminal() {
                cmd_setup(SetupArgs {
                    wallet: None,
                    rpc: None,
                    network: None,
                    database: None,
                    yes: false,
                })
            } else {
                cmd_init(args)
            }
        }
        Commands::Quickstart(args) => cmd_quickstart(args),
        Commands::New(args) => cmd_new(args),
        Commands::Database { command } | Commands::Alias { command } => cmd_database(command),
        Commands::Open(args) => cmd_open(args),
        Commands::Query(args) => {
            let sql = required_sql(args.sql)?;
            let session = build_session(&args.target)?;
            let result = query_typed(&session, &sql)?;
            print_result(&result, OutputMode::Table, true)
        }
        Commands::Exec(args) => {
            let sql = required_sql(args.sql)?;
            let session = build_session(&args.target)?;
            let result = exec_sql(&session, &sql, args.no_wait)?;
            print_exec_result(&result)
        }
        Commands::Tables(args) => {
            let session = build_session(&args)?;
            let result = query_typed(
                &session,
                "select name from sqlite_master where type='table' order by name;",
            )?;
            print_result(&result, OutputMode::Table, true)
        }
        Commands::Schema(args) => {
            let session = build_session(&args)?;
            let result = view(&session, "schema_typed", vec![])?;
            print_result(&result, OutputMode::Table, true)
        }
        Commands::Storage(args) => {
            let session = build_session(&args)?;
            let result = view(&session, "storage_info", vec![])?;
            print_json(&result)
        }
        Commands::Circle(args) => {
            let session = build_session(&args)?;
            let result = program_info(&session)?;
            print_json(&result)
        }
        Commands::Proof(args) => {
            let session = build_session(&args)?;
            verify(&session, None, false)
        }
        Commands::Verify(args) => {
            let session = build_session(&args.target)?;
            verify(&session, args.expected_hash.as_deref(), args.write_smoke)
        }
        Commands::Status(args) => cmd_status(args, "status"),
        Commands::Doctor(args) => cmd_status(args, "doctor"),
        Commands::Config(args) => cmd_config(args),
        Commands::Deploy(args) => cmd_deploy(args),
        Commands::Install => {
            println!("cargo install --path . --locked");
            println!("octra-sqlite setup");
            println!("octra-sqlite new organization < examples/organization-person.sql");
            println!("octra-sqlite organization \"select * from person;\"");
            println!("octra-sqlite status organization");
            Ok(())
        }
    }
}

fn normalize_args(mut args: Vec<String>) -> Vec<String> {
    const KNOWN: &[&str] = &[
        "setup",
        "init",
        "quickstart",
        "new",
        "database",
        "db",
        "alias",
        "open",
        "query",
        "exec",
        "tables",
        "schema",
        "storage",
        "circle",
        "proof",
        "verify",
        "status",
        "doctor",
        "config",
        "deploy",
        "install",
        "help",
        "--help",
        "-h",
        "--version",
        "-V",
    ];
    if args.len() > 1 && !args[1].starts_with('-') && !KNOWN.contains(&args[1].as_str()) {
        args.insert(1, "open".to_string());
    }
    args
}

fn required_sql(value: Option<String>) -> Result<String> {
    value.ok_or_else(|| anyhow!("--sql is required"))
}

fn cmd_init(args: InitArgs) -> Result<()> {
    let mut config = load_config().unwrap_or_default();
    config.wallet = args
        .wallet
        .map(|p| p.to_string_lossy().to_string())
        .or(config.wallet);
    config.rpc = Some(args.rpc);
    config.network = Some(args.network);
    if let Some(database) = args.database {
        config.default_database = Some(database);
    }
    write_config(&config)?;
    println!("wrote {}", config_path()?.display());
    if let Some(default_database) = &config.default_database {
        println!("default database: {default_database}");
    }
    if let Some(wallet) = &config.wallet {
        println!("wallet: {wallet}");
    }
    Ok(())
}

fn cmd_setup(args: SetupArgs) -> Result<()> {
    let mut config = load_config().unwrap_or_default();
    let interactive = !args.yes && io::stdin().is_terminal();
    if !interactive && !args.yes {
        bail!("setup is interactive; run it in a terminal, pass --yes, or use init with flags");
    }

    println!("Octra SQLite setup");
    let wallet_default = args
        .wallet
        .clone()
        .or_else(|| config.wallet.as_ref().map(PathBuf::from))
        .or_else(discover_wallet_path)
        .unwrap_or_else(|| PathBuf::from("./wallet.json"));
    let wallet_path = if interactive {
        prompt_path("Wallet path", &wallet_default)?
    } else {
        wallet_default
    };
    if !wallet_path.is_file() {
        println!(
            "warning: wallet not found at {}; writes need a funded wallet",
            wallet_path.display()
        );
    }

    let rpc_default = args
        .rpc
        .clone()
        .or_else(|| config.rpc.clone())
        .unwrap_or_else(|| DEFAULT_RPC.to_string());
    let rpc = if interactive {
        prompt_default("RPC", &rpc_default)?
    } else {
        rpc_default
    };

    let network_default = args
        .network
        .clone()
        .or_else(|| config.network.clone())
        .unwrap_or_else(|| DEFAULT_NETWORK.to_string());
    let network = if interactive {
        prompt_default("Network", &network_default)?
    } else {
        network_default
    };

    let database_default = args
        .database
        .clone()
        .or_else(|| config.default_database.clone());
    let database = if interactive {
        prompt_optional("Default database", database_default.as_deref())?
    } else {
        database_default
    };

    config.wallet = Some(wallet_path.to_string_lossy().to_string());
    config.rpc = Some(rpc.clone());
    config.network = Some(network.clone());
    if let Some(database) = database.filter(|value| !value.trim().is_empty()) {
        config.default_database = Some(database);
    }
    write_config(&config)?;
    println!("wrote {}", config_path()?.display());
    println!("wallet: {}", wallet_path.display());
    println!("network: {network}");
    println!("rpc: {rpc}");
    if let Some(default_database) = &config.default_database {
        println!("default database: {default_database}");
    }

    if interactive && prompt_yes_no("Create a sample database now?", false)? {
        let name = prompt_default("Database name", "mydb")?;
        let sample = prompt_default("Sample", "people")?;
        cmd_quickstart(QuickstartArgs {
            name,
            sample,
            build: false,
            wasm: None,
            create_ou: "200000".to_string(),
            rpc: Some(rpc),
            network: Some(network),
            no_wait: false,
            no_default: false,
            wallet: Some(wallet_path),
            caller: None,
            private_key_b64: None,
            public_key_b64: None,
        })?;
    } else {
        println!("next: octra-sqlite quickstart organization");
    }
    Ok(())
}

fn cmd_quickstart(args: QuickstartArgs) -> Result<()> {
    sample_sql(&args.sample)?;
    let config = load_config().unwrap_or_default();
    let rpc = args
        .rpc
        .clone()
        .or_else(|| config.rpc.clone())
        .unwrap_or_else(|| DEFAULT_RPC.to_string());
    let network = args
        .network
        .clone()
        .or_else(|| config.network.clone())
        .unwrap_or_else(|| DEFAULT_NETWORK.to_string());
    let name = args.name.clone();
    cmd_new(NewArgs {
        name: args.name,
        build: args.build,
        wasm: args.wasm,
        create_ou: args.create_ou,
        rpc,
        network,
        no_wait: args.no_wait,
        no_name: false,
        default: !args.no_default,
        sql: None,
        read: None,
        sample: Some(args.sample),
        wallet: args.wallet,
        caller: args.caller,
        private_key_b64: args.private_key_b64,
        public_key_b64: args.public_key_b64,
        sql_args: Vec::new(),
    })?;
    println!("next:");
    println!("  octra-sqlite {name} \".tables\"");
    println!("  octra-sqlite {name} \"select * from people;\"");
    println!("  octra-sqlite {name}");
    Ok(())
}

fn cmd_new(args: NewArgs) -> Result<()> {
    let control_args = TargetArgs {
        target: Some(format!("oct://{}/{}", args.network, DEFAULT_CIRCLE)),
        wallet: args.wallet.clone(),
        rpc: Some(args.rpc.clone()),
        caller: args.caller.clone(),
        private_key_b64: args.private_key_b64.clone(),
        public_key_b64: args.public_key_b64.clone(),
    };
    let control_session = build_session(&control_args)?;

    let created = create_circle(&control_session, &args)?;
    let target_uri = format!("oct://{}/{}", args.network, created.circle);
    if args.no_name {
        println!("database: (not saved)");
    } else {
        println!("database: {}", args.name);
    }
    println!("uri: {target_uri}");
    println!("circle: {}", created.circle);
    println!("wallet: {}", control_session.caller);
    println!(
        "code: {} bytes, hash {}",
        created.code_bytes, created.code_hash
    );
    println!("auth: owner-only writes");
    if let Some(hash) = &created.tx_hash {
        println!("create_tx: {hash}");
        if let Some(confirmation) = &created.confirmation {
            println!(
                "create_status: {}",
                confirmation
                    .get("status")
                    .map(value_to_string)
                    .unwrap_or_else(|| "unknown".to_string())
            );
        }
    }

    let mut init_sql = Vec::new();
    if let Some(path) = &args.read {
        init_sql
            .push(fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?);
    }
    if let Some(sample) = &args.sample {
        init_sql.push(sample_sql(sample)?);
    }
    if let Some(sql) = &args.sql {
        init_sql.push(sql.clone());
    }
    if !args.sql_args.is_empty() {
        init_sql.push(args.sql_args.join(" "));
    }
    if init_sql.is_empty() {
        if let Some(sql) = read_stdin_sql()? {
            init_sql.push(sql);
        }
    }
    if !init_sql.is_empty() {
        let session_args = TargetArgs {
            target: Some(target_uri.clone()),
            wallet: args.wallet.clone(),
            rpc: Some(args.rpc.clone()),
            caller: args.caller.clone(),
            private_key_b64: args.private_key_b64.clone(),
            public_key_b64: args.public_key_b64.clone(),
        };
        let session = build_session(&session_args)?;
        for sql in init_sql {
            let result = exec_sql(&session, &sql, args.no_wait)?;
            if let Some(receipt) = result.get("receipt") {
                ensure_receipt_success(receipt, "initializer SQL")?;
            }
            print_exec_result(&result)?;
        }
    }

    if !args.no_name {
        let mut config = load_config().unwrap_or_default();
        config
            .databases
            .insert(args.name.clone(), target_uri.clone());
        if args.default || config.default_database.is_none() {
            config.default_database = Some(args.name.clone());
        }
        write_config(&config)?;
        println!("saved: yes");
    } else {
        println!("saved: no");
    }

    if args.no_name {
        println!("open: octra-sqlite open {target_uri}");
    } else {
        println!("open: octra-sqlite open {}", args.name);
    }
    println!("status: octra-sqlite status {}", args.name);
    Ok(())
}

fn cmd_status(args: DoctorArgs, label: &str) -> Result<()> {
    let mut report = DoctorReport::default();
    let config_path = config_path()?;
    match load_config() {
        Ok(config) => {
            if config_path.exists() {
                report.ok("config", format!("read {}", config_path.display()));
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
            } else {
                report.warn(
                    "default database",
                    "not set; commands without a database use the bundled proof database",
                );
            }

            let wallet_path = resolve_wallet_path(&args.target, &config);
            match load_wallet(wallet_path.as_deref()) {
                Ok(wallet) => {
                    if let Some(path) = wallet_path {
                        report.ok("wallet", format!("read {}", path.display()));
                    } else if env::var("OCTRA_PRIVATE_KEY_B64").is_ok() {
                        report.ok("wallet", "using OCTRA_PRIVATE_KEY_B64");
                    } else {
                        report.warn(
                            "wallet",
                            "not configured; reads/writes that need signed RPC will be skipped",
                        );
                    }
                    if let Some(caller) = first_string(&[
                        args.target.caller.clone(),
                        wallet.addr,
                        wallet.address,
                        env::var("OCTRA_CALLER").ok(),
                    ]) {
                        report.ok("caller", caller);
                    } else {
                        report.warn("caller", "not found in wallet/env");
                    }
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
    report.finish(label)
}

fn cmd_config(args: ConfigArgs) -> Result<()> {
    let config = load_config().unwrap_or_default();
    let path = config_path()?;
    if args.json {
        return print_json(&json!({
            "config": path,
            "wallet": config.wallet,
            "network": config.network,
            "rpc": config.rpc,
            "default_database": config.default_database,
            "databases": config.databases,
        }));
    }

    println!("config: {}", path.display());
    println!(
        "wallet: {}",
        config.wallet.as_deref().unwrap_or("(not configured)")
    );
    println!(
        "network: {}",
        config.network.as_deref().unwrap_or(DEFAULT_NETWORK)
    );
    println!("rpc: {}", config.rpc.as_deref().unwrap_or(DEFAULT_RPC));
    println!(
        "default database: {}",
        config
            .default_database
            .as_deref()
            .unwrap_or("(not configured)")
    );
    println!("databases: {}", config.databases.len());
    if !config.databases.is_empty() {
        println!("next: octra-sqlite database list");
    } else {
        println!("create: octra-sqlite new organization < examples/organization-person.sql");
    }
    Ok(())
}

#[derive(Default)]
struct DoctorReport {
    failures: usize,
}

impl DoctorReport {
    fn ok(&mut self, label: &str, detail: impl AsRef<str>) {
        println!("ok   {label}: {}", detail.as_ref());
    }

    fn warn(&mut self, label: &str, detail: impl AsRef<str>) {
        println!("warn {label}: {}", detail.as_ref());
    }

    fn fail(&mut self, label: &str, detail: impl AsRef<str>) {
        self.failures += 1;
        println!("fail {label}: {}", detail.as_ref());
    }

    fn finish(self, label: &str) -> Result<()> {
        if self.failures == 0 {
            println!("{label}: ready");
            Ok(())
        } else {
            bail!("{label} found {} issue(s)", self.failures)
        }
    }
}

fn check_bundled_wasm(report: &mut DoctorReport) {
    match resolve_wasm_path(false, None) {
        Ok(path) => match fs::read(&path) {
            Ok(bytes) => {
                let hash = sha256_hex(&bytes);
                if bytes.len() == EXPECTED_WASM_BYTES {
                    report.ok("wasm bytes", format!("{} bytes", bytes.len()));
                } else {
                    report.fail(
                        "wasm bytes",
                        format!(
                            "{} bytes at {}; expected {}",
                            bytes.len(),
                            path.display(),
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
                            path.display()
                        ),
                    );
                }
            }
            Err(error) => report.fail("wasm", format!("reading {}: {error}", path.display())),
        },
        Err(error) => report.fail("wasm", error.to_string()),
    }
}

fn check_release_manifest(report: &mut DoctorReport) {
    let Some(path) = find_project_file(RELEASE_MANIFEST_REL) else {
        report.fail(
            "release manifest",
            format!("missing {RELEASE_MANIFEST_REL}"),
        );
        return;
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            report.fail(
                "release manifest",
                format!("reading {}: {error}", path.display()),
            );
            return;
        }
    };
    let manifest: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            report.fail(
                "release manifest",
                format!("parsing {}: {error}", path.display()),
            );
            return;
        }
    };
    report.ok("release manifest", path.display().to_string());
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

fn check_live_target(report: &mut DoctorReport, session: &Session, expected_hash: &str) {
    report.ok("rpc", &session.rpc);
    match program_info(session) {
        Ok(info) => {
            report.ok("circle", &session.target.circle);
            let version = info
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let code_hash = info
                .get("code_hash")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            report.ok("program version", version);
            if let Some(owner) = program_owner(&info) {
                report.ok("circle owner", owner);
                if owner == session.caller {
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
            } else if expected_hash == EXPECTED_WASM_SHA256 {
                match personalized_wasm_hash(session) {
                    Ok(Some(personalized_hash)) if code_hash == personalized_hash => {
                        report.ok(
                            "program hash",
                            format!("{code_hash} (owner-personalized bundled WASM)"),
                        );
                    }
                    Ok(Some(personalized_hash)) => report.fail(
                        "program hash",
                        format!(
                            "{code_hash}; expected {expected_hash} or owner-personalized {personalized_hash}"
                        ),
                    ),
                    Ok(None) => report.fail(
                        "program hash",
                        format!("{code_hash}; expected {expected_hash}"),
                    ),
                    Err(error) => report.fail(
                        "program hash",
                        format!("{code_hash}; expected {expected_hash}; personalized check failed: {error:#}"),
                    ),
                }
            } else {
                report.fail(
                    "program hash",
                    format!("{code_hash}; expected {expected_hash}"),
                );
            }
        }
        Err(error) => report.fail("program info", error.to_string()),
    }
    match view(session, "storage_info", vec![]) {
        Ok(storage) => report.ok(
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
        ),
        Err(error) => report.fail("storage", error.to_string()),
    }
    match auth_info(session) {
        Ok(auth) => {
            if auth.configured {
                report.ok("auth", "owner write intent");
                if let Some(owner_pubkey) = auth.owner_pubkey.as_deref() {
                    report.ok("auth owner pubkey", owner_pubkey);
                    match intent_public_key(session) {
                        Ok(wallet_pubkey) if hex::encode(wallet_pubkey) == owner_pubkey => {
                            report.ok("auth owner wallet", "current wallet can write")
                        }
                        Ok(_) => report.warn("auth owner wallet", "current wallet is read-only"),
                        Err(error) => report.warn(
                            "auth owner wallet",
                            format!("could not derive wallet public key: {error:#}"),
                        ),
                    }
                }
                report.ok("auth db id", &auth.db_id);
                if let Some(sequence) = auth.owner_sequence {
                    report.ok("auth sequence", sequence.to_string());
                }
            } else {
                report.warn("auth", "unconfigured bundled WASM; writes are unsigned");
            }
        }
        Err(error) => report.fail("auth info", error.to_string()),
    }
    match query_typed(session, "select sqlite_version() as sqlite_version;") {
        Ok(result) => report.ok("typed query", value_to_string(&result)),
        Err(error) => report.fail("typed query", error.to_string()),
    }
}

fn program_owner(info: &Value) -> Option<&str> {
    ["owner", "program_owner", "creator", "deployer"]
        .into_iter()
        .find_map(|key| info.get(key).and_then(Value::as_str))
}

fn personalized_wasm_hash(session: &Session) -> Result<Option<String>> {
    let auth = auth_info(session)?;
    if !auth.configured {
        return Ok(None);
    }
    let owner_pubkey = hex_to_32(
        "owner_pubkey",
        auth.owner_pubkey
            .as_deref()
            .ok_or_else(|| anyhow!("auth_info missing owner_pubkey"))?,
    )?;
    let db_id = hex_to_32("db_id", &auth.db_id)?;
    let wasm_path = resolve_wasm_path(false, None)?;
    let mut wasm =
        fs::read(&wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;
    patch_wasm_auth_bytes(&mut wasm, &owner_pubkey, &db_id)?;
    Ok(Some(sha256_hex(&wasm)))
}

fn hex_to_32(label: &str, text: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(text).with_context(|| format!("decoding {label} hex"))?;
    if bytes.len() != 32 {
        bail!("{label} must decode to 32 bytes");
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

struct CreatedCircle {
    circle: String,
    code_hash: String,
    code_bytes: usize,
    tx_hash: Option<String>,
    confirmation: Option<Value>,
}

#[derive(Clone, Debug)]
struct AuthPatch {
    owner_pubkey_hex: String,
    db_id_hex: String,
    owner_pubkey_offset: usize,
    db_id_offset: usize,
}

struct AuthInfo {
    configured: bool,
    db_id: String,
    owner_pubkey: Option<String>,
    owner_sequence: Option<u64>,
}

fn create_circle(session: &Session, args: &NewArgs) -> Result<CreatedCircle> {
    let wasm_path = resolve_wasm_for_new(args)?;
    let mut wasm =
        fs::read(&wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;
    patch_wasm_auth_for_owner(&mut wasm, session)?;
    let code_hash = sha256_hex(&wasm);
    let code_b64 = general_purpose::STANDARD.encode(&wasm);
    let payload_json = circle_deploy_payload_json(Some(&code_b64))?;
    let nonce = next_nonce(session)?;
    let circle = circle_id_of_deploy(&session.caller, nonce as u64, &payload_json);
    let tx = Tx {
        from: session.caller.clone(),
        to_: circle.clone(),
        amount: "0".to_string(),
        nonce,
        ou: args.create_ou.clone(),
        timestamp: now_timestamp(),
        op_type: "deploy_circle".to_string(),
        encrypted_data: String::new(),
        message: payload_json,
        signature: String::new(),
        public_key: session.public_key_b64.clone(),
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
    let mut circle_session = session.clone();
    circle_session.target = Target {
        raw: format!("oct://{}/{}", args.network, circle),
        network: args.network.clone(),
        circle: circle.clone(),
        rpc: session.rpc.clone(),
    };
    if !args.no_wait {
        wait_for_program_info(&circle_session, &code_hash)?;
    }
    Ok(CreatedCircle {
        circle,
        code_hash,
        code_bytes: wasm.len(),
        tx_hash,
        confirmation,
    })
}

fn patch_wasm_auth_for_owner(wasm: &mut [u8], session: &Session) -> Result<AuthPatch> {
    let owner_pubkey = intent_public_key(session)?;
    let db_id = derive_db_id(session, &owner_pubkey);
    patch_wasm_auth_bytes(wasm, &owner_pubkey, &db_id)
}

fn patch_wasm_auth_bytes(
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

fn patch_wasm_auth_from_info(wasm: &mut [u8], auth: &AuthInfo) -> Result<AuthPatch> {
    if !auth.configured {
        bail!("auth_info reports unconfigured owner write intent auth");
    }
    let owner_pubkey = hex_to_32(
        "owner_pubkey",
        auth.owner_pubkey
            .as_deref()
            .ok_or_else(|| anyhow!("auth_info missing owner_pubkey"))?,
    )?;
    let db_id = hex_to_32("db_id", &auth.db_id)?;
    patch_wasm_auth_bytes(wasm, &owner_pubkey, &db_id)
}

fn replace_wasm_placeholder(
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

fn derive_db_id(session: &Session, owner_pubkey: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"octra-sqlite.db-id.v1");
    hasher.update(session.caller.as_bytes());
    hasher.update(owner_pubkey);
    hasher.update(now_timestamp().to_string().as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn resolve_wasm_for_new(args: &NewArgs) -> Result<PathBuf> {
    resolve_wasm_path(args.build, args.wasm.as_deref())
}

fn resolve_wasm_path(build: bool, wasm: Option<&Path>) -> Result<PathBuf> {
    if build {
        build_wasm_from_checkout()?;
    }
    if let Some(path) = wasm {
        return require_file(path.to_path_buf(), "custom WASM");
    }
    if let Ok(path) = env::var("OCTRA_SQLITE_WASM") {
        return require_file(PathBuf::from(path), "OCTRA_SQLITE_WASM");
    }
    if let Some(path) = find_project_file(DEFAULT_WASM_REL) {
        return Ok(path);
    }
    bail!(
        "could not find bundled {DEFAULT_WASM_REL}; restore the repo artifact, pass --wasm, set OCTRA_SQLITE_WASM, or pass --build from a checkout with a WASI clang"
    )
}

fn require_file(path: PathBuf, label: &str) -> Result<PathBuf> {
    if path.is_file() {
        Ok(path)
    } else {
        bail!(
            "{label} does not exist or is not a file: {}",
            path.display()
        )
    }
}

fn build_wasm_from_checkout() -> Result<()> {
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

fn find_project_file(relative: &str) -> Option<PathBuf> {
    for root in project_roots() {
        let path = root.join(relative);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn project_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd);
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    let mut unique = Vec::new();
    for root in roots {
        if !unique.iter().any(|existing: &PathBuf| existing == &root) {
            unique.push(root);
        }
    }
    unique
}

fn ensure_receipt_success(receipt: &Value, label: &str) -> Result<()> {
    if receipt.get("success").and_then(Value::as_bool) == Some(false) {
        if let Some(error) = receipt.get("error").filter(|value| !value.is_null()) {
            bail!("{label} failed: {error}");
        }
        bail!("{label} failed");
    }
    Ok(())
}

fn circle_deploy_payload_json(code_b64: Option<&str>) -> Result<String> {
    let code = match code_b64 {
        Some(value) => serde_json::to_string(value)?,
        None => "null".to_string(),
    };
    Ok(format!(
        "{{\"runtime\":\"wasm_v1\",\"privacy_class\":\"sealed\",\"browser_mode\":\"native_sealed\",\"resource_mode\":\"sealed_read\",\"code_b64\":{},\"policy_hash\":null,\"members_root\":null,\"export_policy\":null,\"limits\":{{\"max_stable_bytes\":\"33554432\",\"max_assets_bytes\":\"33554432\",\"max_inline_value\":\"65536\",\"max_wasm_bytes\":\"33554432\"}}}}",
        code
    ))
}

fn circle_id_of_deploy(deployer: &str, nonce: u64, payload_json: &str) -> String {
    let payload_hash = h256_hex_frame("octra:circle_deploy_payload:v1", &[payload_json.as_bytes()]);
    let nonce_bytes = nonce.to_be_bytes();
    let seed = h256_raw_frame(
        "octra:circle_deploy_id:v1",
        &[deployer.as_bytes(), &nonce_bytes, payload_hash.as_bytes()],
    );
    let base58 = base58_encode(&seed);
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

fn h256_raw_frame(tag: &str, parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(tag.as_bytes());
    hasher.update([0]);
    for part in parts {
        hasher.update((part.len() as u32).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn h256_hex_frame(tag: &str, parts: &[&[u8]]) -> String {
    hex::encode(h256_raw_frame(tag, parts))
}

fn base58_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    if bytes.iter().all(|byte| *byte == 0) {
        return "1".repeat(bytes.len());
    }

    let mut digits = vec![0u8];
    for byte in bytes {
        let mut carry = u32::from(*byte);
        for digit in &mut digits {
            let value = u32::from(*digit) * 256 + carry;
            *digit = (value % 58) as u8;
            carry = value / 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }

    let mut encoded = String::new();
    for byte in bytes {
        if *byte == 0 {
            encoded.push('1');
        } else {
            break;
        }
    }
    for digit in digits.iter().rev() {
        encoded.push(ALPHABET[usize::from(*digit)] as char);
    }
    encoded
}

fn cmd_database(command: DatabaseCommand) -> Result<()> {
    let mut config = load_config().unwrap_or_default();
    match command {
        DatabaseCommand::List => print_database_list(&config),
        DatabaseCommand::Info { database } => print_database_info(&config, database.as_deref())?,
        DatabaseCommand::Set { name, database } => {
            parse_target_uri(&database, &config)?;
            config.databases.insert(name.clone(), database.clone());
            if config.default_database.is_none() {
                config.default_database = Some(name.clone());
            }
            write_config(&config)?;
            println!("{name} -> {database}");
            println!("open: octra-sqlite {name}");
        }
        DatabaseCommand::Use { name } => {
            if !config.databases.contains_key(&name) {
                bail!("unknown database {name}; run octra-sqlite database list");
            }
            config.default_database = Some(name.clone());
            write_config(&config)?;
            println!("default database: {name}");
            println!("open: octra-sqlite");
        }
        DatabaseCommand::Remove { name } => {
            config.databases.remove(&name);
            if config.default_database.as_deref() == Some(&name) {
                config.default_database = None;
            }
            write_config(&config)?;
            println!("removed {name}");
        }
    }
    Ok(())
}

fn print_database_list(config: &Config) {
    if config.databases.is_empty() {
        println!("no databases");
        println!("create: octra-sqlite new organization < examples/organization-person.sql");
        return;
    }
    println!("default  name  uri");
    println!("-------  ----  ---");
    for (name, database) in &config.databases {
        let default_mark = if config.default_database.as_deref() == Some(name) {
            "*"
        } else {
            ""
        };
        println!("{default_mark:<7}  {name}  {database}");
    }
}

fn print_database_info(config: &Config, database: Option<&str>) -> Result<()> {
    let requested = database
        .map(str::to_string)
        .or_else(|| config.default_database.clone())
        .ok_or_else(|| anyhow!("no database supplied and no default database is configured"))?;
    let saved_uri = config.databases.get(&requested);
    let target = resolve_target(&requested, config)?;
    println!(
        "name: {}",
        if saved_uri.is_some() {
            requested.as_str()
        } else {
            "(not saved)"
        }
    );
    println!(
        "default: {}",
        config.default_database.as_deref() == Some(requested.as_str())
    );
    println!("uri: {}", target.raw);
    println!("network: {}", target.network);
    println!("circle: {}", target.circle);
    println!("rpc: {}", target.rpc);
    println!("open: octra-sqlite {}", requested);
    println!("status: octra-sqlite status {}", requested);
    Ok(())
}

fn cmd_open(args: OpenArgs) -> Result<()> {
    let session = build_session(&args.target)?;
    let mode = if args.json {
        OutputMode::Json
    } else {
        OutputMode::Table
    };
    if args.sql.is_empty() {
        if let Some(sql) = read_stdin_sql()? {
            return run_one_sql(&session, &sql, mode, true);
        }
        run_shell(session, mode)
    } else {
        let sql = args.sql.join(" ");
        run_one_sql(&session, &sql, mode, true)
    }
}

fn read_stdin_sql() -> Result<Option<String>> {
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

fn run_one_sql(session: &Session, sql: &str, mode: OutputMode, headers: bool) -> Result<()> {
    run_one_sql_to(session, sql, mode, headers, None)
}

fn run_one_sql_to(
    session: &Session,
    sql: &str,
    mode: OutputMode,
    headers: bool,
    output: Option<&Path>,
) -> Result<()> {
    let trimmed = sql.trim();
    if trimmed.starts_with('.') && !trimmed.contains('\n') {
        let mut state = ShellState {
            session: session.clone(),
            mode,
            headers,
            timer: false,
            output: output.map(Path::to_path_buf),
        };
        handle_dot_command(&mut state, trimmed)?;
        return Ok(());
    }
    if is_read_sql(sql) {
        let result = query_typed(session, sql)?;
        write_text(output, &format_result(&result, mode, headers)?)
    } else {
        let result = exec_sql(session, sql, false)?;
        if mode == OutputMode::Json {
            write_text(output, &format_json(&result)?)
        } else {
            write_text(output, &format_exec_result(&result)?)
        }
    }
}

fn run_shell(session: Session, mode: OutputMode) -> Result<()> {
    println!("SQLite on Octra ({})", session.target.network);
    println!("circle: {}", session.target.circle);
    println!("wallet: {}", session.caller);
    println!("type .help for usage");
    let mut state = ShellState {
        session,
        mode,
        headers: true,
        timer: false,
        output: None,
    };
    let mut buffer = String::new();
    loop {
        let prompt = if buffer.trim().is_empty() {
            "sqlite> "
        } else {
            "   ...> "
        };
        print!("{prompt}");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if buffer.trim().is_empty() && trimmed.starts_with('.') {
            if handle_dot_command(&mut state, trimmed)? {
                break;
            }
            continue;
        }
        buffer.push_str(&line);
        if trimmed.ends_with(';') {
            let sql = buffer.trim().to_string();
            buffer.clear();
            let started = Instant::now();
            if let Err(error) = run_one_sql_to(
                &state.session,
                &sql,
                state.mode,
                state.headers,
                state.output.as_deref(),
            ) {
                eprintln!("error: {error:#}");
            }
            if state.timer {
                println!("Run Time: real {:.3}", started.elapsed().as_secs_f64());
            }
        }
    }
    Ok(())
}

fn handle_dot_command(state: &mut ShellState, line: &str) -> Result<bool> {
    let mut parts = line.split_whitespace();
    let cmd = parts.next().unwrap_or("");
    match cmd {
        ".quit" | ".exit" => return Ok(true),
        ".help" => print_help(),
        ".mode" => {
            let mode = parts
                .next()
                .ok_or_else(|| anyhow!("usage: .mode box|table|list|json|line|csv"))?;
            state.mode = match mode {
                "box" | "table" => OutputMode::Table,
                "list" => OutputMode::List,
                "json" => OutputMode::Json,
                "line" => OutputMode::Line,
                "csv" => OutputMode::Csv,
                _ => bail!("unknown mode {mode}"),
            };
        }
        ".headers" => {
            let value = parts
                .next()
                .ok_or_else(|| anyhow!("usage: .headers on|off"))?;
            state.headers = match value {
                "on" => true,
                "off" => false,
                _ => bail!("usage: .headers on|off"),
            };
        }
        ".tables" => {
            let result = query_typed(
                &state.session,
                "select name from sqlite_master where type='table' order by name;",
            )?;
            write_text(
                state.output.as_deref(),
                &format_result(&result, state.mode, state.headers)?,
            )?;
        }
        ".schema" => {
            let result = view(&state.session, "schema_typed", vec![])?;
            write_text(
                state.output.as_deref(),
                &format_result(&result, state.mode, state.headers)?,
            )?;
        }
        ".show" => print_shell_show(state),
        ".databases" => print_current_database(&state.session),
        ".storage" => write_text(
            state.output.as_deref(),
            &format_json(&view(&state.session, "storage_info", vec![])?)?,
        )?,
        ".circle" => write_text(
            state.output.as_deref(),
            &format_json(&program_info(&state.session)?)?,
        )?,
        ".wallet" => {
            println!("{}", state.session.caller);
            if let Some(path) = &state.session.wallet_path {
                println!("wallet: {}", path.display());
            }
        }
        ".proof" | ".verify" => verify(&state.session, None, false)?,
        ".read" => {
            let path = parts.next().ok_or_else(|| anyhow!("usage: .read FILE"))?;
            let sql = fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
            run_one_sql_to(
                &state.session,
                &sql,
                state.mode,
                state.headers,
                state.output.as_deref(),
            )?;
        }
        ".timer" => {
            let value = parts
                .next()
                .ok_or_else(|| anyhow!("usage: .timer on|off"))?;
            state.timer = match value {
                "on" => true,
                "off" => false,
                _ => bail!("usage: .timer on|off"),
            };
        }
        ".output" => {
            let value = parts
                .next()
                .ok_or_else(|| anyhow!("usage: .output FILE|stdout"))?;
            if value == "stdout" || value == "off" {
                state.output = None;
            } else {
                fs::write(value, "").with_context(|| format!("opening output {value}"))?;
                state.output = Some(PathBuf::from(value));
            }
        }
        ".open" => {
            let target = parts
                .next()
                .ok_or_else(|| anyhow!("usage: .open DATABASE"))?;
            let args = TargetArgs {
                target: Some(target.to_string()),
                wallet: state.session.wallet_path.clone(),
                rpc: Some(state.session.rpc.clone()),
                caller: Some(state.session.caller.clone()),
                private_key_b64: Some(state.session.private_key_text.clone()),
                public_key_b64: Some(state.session.public_key_b64.clone()),
            };
            state.session = build_session(&args)?;
            println!("circle: {}", state.session.target.circle);
        }
        _ => bail!("unknown command {cmd}; try .help"),
    }
    Ok(false)
}

fn print_help() {
    println!("SQLite commands:");
    println!("  .tables              list tables");
    println!("  .schema              show schema");
    println!("  .databases           show the current main database URI");
    println!("  .open DATABASE       switch database name, Circle id, or oct:// URI");
    println!("  .read FILE           execute SQL from a file");
    println!("  .mode MODE           MODE is box, table, list, json, line, or csv");
    println!("  .headers on|off      show or hide column headers");
    println!("  .timer on|off        show query timing");
    println!("  .output FILE|stdout  redirect output");
    println!("  .show                show shell settings");
    println!("  .quit                exit");
    println!();
    println!("Octra commands:");
    println!("  .storage             show SQLite page storage info");
    println!("  .circle              show Circle program metadata");
    println!("  .wallet              show active wallet address");
    println!("  .proof               prove live Circle SQLite status");
    println!("  .verify              same as .proof");
}

fn print_current_database(session: &Session) {
    println!("seq  name  file");
    println!("---  ----  ----");
    println!("0    main  {}", session.target.raw);
}

fn print_shell_show(state: &ShellState) {
    println!("database: {}", state.session.target.raw);
    println!("circle: {}", state.session.target.circle);
    println!("network: {}", state.session.target.network);
    println!("rpc: {}", state.session.rpc);
    println!("wallet: {}", state.session.caller);
    println!("mode: {}", state.mode.name());
    println!("headers: {}", if state.headers { "on" } else { "off" });
    println!("timer: {}", if state.timer { "on" } else { "off" });
    println!(
        "output: {}",
        state
            .output
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "stdout".to_string())
    );
}

fn resolve_wallet_path(args: &TargetArgs, config: &Config) -> Option<PathBuf> {
    args.wallet
        .clone()
        .or_else(|| env::var("OCTRA_WALLET").ok().map(PathBuf::from))
        .or_else(|| config.wallet.as_ref().map(PathBuf::from))
        .or_else(discover_wallet_path)
}

fn build_session(args: &TargetArgs) -> Result<Session> {
    let config = load_config().unwrap_or_default();
    let target_value = args
        .target
        .clone()
        .or_else(|| config.default_database.clone())
        .or_else(|| env::var("OCTRA_SQLITE_DATABASE").ok())
        .or_else(|| env::var("OCTRA_SQLITE_TARGET").ok())
        .or_else(|| env::var("OCTRA_CIRCLE_ID").ok())
        .unwrap_or_else(|| DEFAULT_CIRCLE.to_string());
    let mut target = resolve_target(&target_value, &config)?;
    if let Some(rpc) = args
        .rpc
        .clone()
        .or_else(|| env::var("OCTRA_RPC_URL").ok())
        .or_else(|| config.rpc.clone())
    {
        target.rpc = rpc;
    }
    let wallet_path = resolve_wallet_path(args, &config);
    let wallet = load_wallet(wallet_path.as_deref())?;
    let rpc = first_string(&[
        args.rpc.clone(),
        wallet.rpc.clone(),
        Some(target.rpc.clone()),
        config.rpc.clone(),
        Some(DEFAULT_RPC.to_string()),
    ])
    .unwrap();
    let caller = first_string(&[
        args.caller.clone(),
        wallet.addr.clone(),
        wallet.address.clone(),
        env::var("OCTRA_CALLER").ok(),
        Some(DEFAULT_CALLER.to_string()),
    ])
    .ok_or_else(|| anyhow!("caller is required"))?;
    let private_key_text = first_string(&[
        args.private_key_b64.clone(),
        wallet.priv_field.clone(),
        wallet.priv_.clone(),
        wallet.private_key.clone(),
        wallet.private_key_b64.clone(),
        env::var("OCTRA_PRIVATE_KEY_B64").ok(),
    ])
    .ok_or_else(|| {
        anyhow!("wallet private key is required; pass --wallet or OCTRA_PRIVATE_KEY_B64")
    })?;
    let signing_key = signing_key_from_text(&private_key_text)?;
    let derived_pub = general_purpose::STANDARD.encode(signing_key.verifying_key().to_bytes());
    let public_key_b64 = first_string(&[
        args.public_key_b64.clone(),
        wallet.pub_field.clone(),
        wallet.pub_.clone(),
        wallet.public_key.clone(),
        wallet.public_key_b64.clone(),
        env::var("OCTRA_PUBLIC_KEY_B64").ok(),
        Some(derived_pub),
    ])
    .unwrap();
    Ok(Session {
        target,
        wallet_path,
        rpc,
        caller,
        private_key_text,
        public_key_b64,
    })
}

fn load_wallet(path: Option<&Path>) -> Result<WalletFile> {
    match path {
        Some(path) => {
            let text = fs::read_to_string(path)
                .with_context(|| format!("reading wallet {}", path.display()))?;
            Ok(serde_json::from_str(&text)
                .with_context(|| format!("parsing wallet {}", path.display()))?)
        }
        None => Ok(WalletFile::default()),
    }
}

fn discover_wallet_path() -> Option<PathBuf> {
    wallet_candidates().into_iter().find(|path| path.is_file())
}

fn wallet_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("wallet.json"));
        candidates.push(cwd.join(".octra").join("wallet.json"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".octra").join("wallet.json"));
        candidates.push(home.join(".octra").join("devnet-wallet.json"));
    }
    candidates
}

fn first_string(values: &[Option<String>]) -> Option<String> {
    values
        .iter()
        .find_map(|value| value.as_ref().filter(|v| !v.is_empty()).cloned())
}

fn sample_sql(name: &str) -> Result<String> {
    match name {
        "people" => Ok(
            "create table people(first_name text not null, last_name text not null);\n\
insert into people(first_name,last_name)\n\
values ('Ada','Lovelace'),('Grace','Hopper'),('Katherine','Johnson');\n"
                .to_string(),
        ),
        _ => bail!("unknown sample {name}; available samples: people"),
    }
}

fn prompt_default(label: &str, default: &str) -> Result<String> {
    print!("{label} [{default}]: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn prompt_optional(label: &str, default: Option<&str>) -> Result<Option<String>> {
    let rendered = default.unwrap_or("");
    print!("{label}");
    if !rendered.is_empty() {
        print!(" [{rendered}]");
    }
    print!(": ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Ok(default
            .map(str::to_string)
            .filter(|value| !value.is_empty()))
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn prompt_path(label: &str, default: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(prompt_default(
        label,
        &default.to_string_lossy(),
    )?))
}

fn prompt_yes_no(label: &str, default: bool) -> Result<bool> {
    let default_text = if default { "Y/n" } else { "y/N" };
    print!("{label} [{default_text}]: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Ok(default);
    }
    match trimmed.as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => bail!("answer yes or no"),
    }
}

fn resolve_target(value: &str, config: &Config) -> Result<Target> {
    if let Some(database) = config.databases.get(value) {
        return resolve_target(database, config);
    }
    parse_target_uri(value, config)
}

fn parse_target_uri(value: &str, config: &Config) -> Result<Target> {
    let default_rpc = config
        .rpc
        .clone()
        .unwrap_or_else(|| DEFAULT_RPC.to_string());
    let default_network = config
        .network
        .clone()
        .unwrap_or_else(|| DEFAULT_NETWORK.to_string());
    if let Some(rest) = value.strip_prefix("oct://") {
        let without_query = rest.split('?').next().unwrap_or(rest);
        let pieces: Vec<&str> = without_query
            .trim_matches('/')
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();
        let (network, circle) = match pieces.as_slice() {
            [circle] => (default_network, (*circle).to_string()),
            [network, circle] => ((*network).to_string(), (*circle).to_string()),
            _ => bail!("oct database URI must look like oct://NETWORK/<circle-id>"),
        };
        if !circle.starts_with("oct") {
            bail!("circle id must start with oct");
        }
        return Ok(Target {
            raw: value.to_string(),
            network,
            circle,
            rpc: default_rpc,
        });
    }
    if value.starts_with("oct") {
        return Ok(Target {
            raw: value.to_string(),
            network: default_network,
            circle: value.to_string(),
            rpc: default_rpc,
        });
    }
    bail!("unknown database {value}; use a database name, Circle id, or oct://NETWORK/<circle-id>")
}

fn config_path() -> Result<PathBuf> {
    if let Ok(path) = env::var("OCTRA_SQLITE_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not locate home directory"))?;
    Ok(home.join(".octra").join("sqlite.json"))
}

fn load_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?)
}

fn write_config(config: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(config)? + "\n")?;
    Ok(())
}

fn signing_key_from_text(text: &str) -> Result<SigningKey> {
    let cleaned = text.trim();
    let raw = general_purpose::STANDARD
        .decode(cleaned)
        .or_else(|_| hex::decode(cleaned))
        .map_err(|_| anyhow!("private key must be base64 or hex"))?;
    if raw.len() < 32 {
        bail!("private key must decode to at least 32 bytes");
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&raw[..32]);
    Ok(SigningKey::from_bytes(&seed))
}

fn intent_signing_key(session: &Session) -> Result<SigningKey> {
    signing_key_from_text(&session.private_key_text)
}

fn intent_public_key(session: &Session) -> Result<[u8; 32]> {
    Ok(intent_signing_key(session)?.verifying_key().to_bytes())
}

fn sign_b64(session: &Session, message: &str) -> Result<String> {
    let signing_key = signing_key_from_text(&session.private_key_text)?;
    Ok(general_purpose::STANDARD.encode(signing_key.sign(message.as_bytes()).to_bytes()))
}

fn sign_bytes_hex(session: &Session, message: &[u8]) -> Result<String> {
    let signing_key = intent_signing_key(session)?;
    Ok(hex::encode(signing_key.sign(message).to_bytes()))
}

fn rpc_call(session: &Session, method: &str, params: Value) -> Result<Value> {
    let client = reqwest::blocking::Client::new();
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });
    let response = client
        .post(&session.rpc)
        .json(&body)
        .send()
        .with_context(|| format!("calling {method}"))?;
    let status = response.status();
    let payload: Value = response
        .json()
        .with_context(|| format!("decoding {method} response"))?;
    if !status.is_success() {
        bail!("{method} failed with HTTP {status}: {payload}");
    }
    if let Some(error) = payload.get("error") {
        bail!("{method} failed: {error}");
    }
    Ok(payload.get("result").cloned().unwrap_or(Value::Null))
}

fn compact_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn view(session: &Session, method: &str, params: Vec<Value>) -> Result<Value> {
    let params_value = Value::Array(params.clone());
    let params_json = compact_json(&params_value)?;
    let params_hash = sha256_hex(params_json.as_bytes());
    let message = format!(
        "octra_circle_view|{}|{}|{}|{}|0",
        session.target.circle, session.caller, method, params_hash
    );
    let signature = sign_b64(session, &message)?;
    let result = rpc_call(
        session,
        "octra_circleViewAuth",
        json!([
            session.target.circle,
            method,
            params,
            session.caller,
            session.public_key_b64,
            signature,
            false
        ]),
    )?;
    decode_rpc_result(result)
}

fn query_typed(session: &Session, sql: &str) -> Result<Value> {
    view(session, "query_typed", vec![Value::String(sql.to_string())])
}

fn auth_info(session: &Session) -> Result<AuthInfo> {
    let value = view(session, "auth_info", vec![])?;
    let configured = value
        .get("configured")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let db_id = value
        .get("db_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("auth_info missing db_id"))?
        .to_string();
    let owner_pubkey = value
        .get("owner_pubkey")
        .and_then(Value::as_str)
        .map(str::to_string);
    let owner_sequence = value.get("owner_sequence").and_then(Value::as_u64);
    Ok(AuthInfo {
        configured,
        db_id,
        owner_pubkey,
        owner_sequence,
    })
}

fn program_info(session: &Session) -> Result<Value> {
    let message = format!(
        "octra_circle_program_info|{}|{}",
        session.target.circle, session.caller
    );
    let signature = sign_b64(session, &message)?;
    rpc_call(
        session,
        "octra_circleProgramInfoAuth",
        json!([
            session.target.circle,
            session.caller,
            session.public_key_b64,
            signature
        ]),
    )
}

fn exec_sql(session: &Session, sql: &str, no_wait: bool) -> Result<Value> {
    let nonce = next_nonce(session)?;
    let timestamp = now_timestamp();
    let trace_sql = trace_sql_event_enabled();
    let method = if trace_sql { "exec_trace" } else { "exec" };
    let auth = auth_info(session).with_context(|| {
        "could not read Circle auth_info; refusing to choose unsigned exec implicitly"
    })?;
    let params = if auth.configured {
        signed_exec_params(session, &auth, nonce as u64, method, sql)?
    } else {
        vec![Value::String(sql.to_string())]
    };
    let message = compact_json(&Value::Array(params))?;
    let tx = Tx {
        from: session.caller.clone(),
        to_: session.target.circle.clone(),
        amount: "0".to_string(),
        nonce,
        ou: "1000".to_string(),
        timestamp,
        op_type: "circle_call".to_string(),
        encrypted_data: method.to_string(),
        message,
        signature: String::new(),
        public_key: session.public_key_b64.clone(),
    };
    submit_tx(session, tx, no_wait)
}

fn signed_exec_params(
    session: &Session,
    info: &AuthInfo,
    sequence: u64,
    method: &str,
    sql: &str,
) -> Result<Vec<Value>> {
    let db_id = hex_to_32("db_id", &info.db_id)?;
    let pubkey_hex = hex::encode(intent_public_key(session)?);
    let sequence_text = sequence.to_string();
    let message = owner_write_intent_message(&db_id, sequence, method, sql)?;
    let sig_hex = sign_bytes_hex(session, &message)?;
    Ok(vec![
        Value::String(sql.to_string()),
        Value::String(pubkey_hex),
        Value::String(sequence_text),
        Value::String(sig_hex),
    ])
}

fn owner_write_intent_message(
    db_id: &[u8; 32],
    sequence: u64,
    method: &str,
    sql: &str,
) -> Result<Vec<u8>> {
    if method.is_empty() || method.len() > 16 {
        bail!("owner write intent method must be 1..16 bytes");
    }
    if sql.len() > u32::MAX as usize {
        bail!("owner write intent SQL is too large");
    }
    let mut message = Vec::with_capacity(
        OWNER_WRITE_INTENT_DOMAIN.len() + 32 + 8 + 2 + method.len() + 4 + sql.len(),
    );
    message.extend_from_slice(OWNER_WRITE_INTENT_DOMAIN);
    message.extend_from_slice(db_id);
    message.extend_from_slice(&sequence.to_be_bytes());
    message.extend_from_slice(&(method.len() as u16).to_be_bytes());
    message.extend_from_slice(method.as_bytes());
    message.extend_from_slice(&(sql.len() as u32).to_be_bytes());
    message.extend_from_slice(sql.as_bytes());
    Ok(message)
}

fn trace_sql_event_enabled() -> bool {
    env::var("OCTRA_SQLITE_TRACE_SQL_EVENT")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn next_nonce(session: &Session) -> Result<i64> {
    let balance = rpc_call(session, "octra_balance", json!([session.caller]))?;
    Ok(balance
        .get("pending_nonce")
        .or_else(|| balance.get("nonce"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        + 1)
}

fn submit_tx(session: &Session, mut tx: Tx, no_wait: bool) -> Result<Value> {
    let canonical = canonical_tx(&tx);
    tx.signature = sign_b64(session, &canonical)?;
    let tx_circle = tx.to_.clone();
    let tx_wallet = tx.from.clone();
    let result = rpc_call(session, "octra_submit", json!([tx]))?;
    let tx_hash = result
        .get("tx_hash")
        .or_else(|| result.get("hash"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut out = Map::new();
    out.insert("circle".to_string(), Value::String(tx_circle));
    out.insert("wallet".to_string(), Value::String(tx_wallet));
    out.insert("result".to_string(), result);
    if let Some(hash) = tx_hash.clone() {
        out.insert("tx_hash".to_string(), Value::String(hash.clone()));
        if !no_wait {
            let receipt = wait_for_receipt(session, &hash)?;
            out.insert("receipt".to_string(), receipt);
        }
    }
    Ok(Value::Object(out))
}

#[derive(Serialize)]
struct Tx {
    from: String,
    to_: String,
    amount: String,
    nonce: i64,
    ou: String,
    timestamp: f64,
    op_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    encrypted_data: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    message: String,
    signature: String,
    public_key: String,
}

fn now_timestamp() -> f64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_secs() as f64 + f64::from(duration.subsec_millis()) / 1000.0
}

fn canonical_timestamp(value: f64) -> String {
    let mut text = serde_json::to_string(&value).unwrap_or_else(|_| format!("{value}"));
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        text.push_str(".0");
    }
    text
}

fn canonical_tx(tx: &Tx) -> String {
    let mut s = String::new();
    s.push_str("{\"from\":\"");
    s.push_str(&escape_json_string(&tx.from));
    s.push_str("\",\"to_\":\"");
    s.push_str(&escape_json_string(&tx.to_));
    s.push_str("\",\"amount\":\"");
    s.push_str(&escape_json_string(&tx.amount));
    s.push_str("\",\"nonce\":");
    s.push_str(&tx.nonce.to_string());
    s.push_str(",\"ou\":\"");
    s.push_str(&escape_json_string(&tx.ou));
    s.push_str("\",\"timestamp\":");
    s.push_str(&canonical_timestamp(tx.timestamp));
    s.push_str(",\"op_type\":\"");
    s.push_str(&escape_json_string(&tx.op_type));
    s.push('"');
    if !tx.encrypted_data.is_empty() {
        s.push_str(",\"encrypted_data\":\"");
        s.push_str(&escape_json_string(&tx.encrypted_data));
        s.push('"');
    }
    if !tx.message.is_empty() {
        s.push_str(",\"message\":\"");
        s.push_str(&escape_json_string(&tx.message));
        s.push('"');
    }
    s.push('}');
    s
}

fn escape_json_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\u{0008}', "\\b")
        .replace('\u{000c}', "\\f")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn wait_for_receipt(session: &Session, tx_hash: &str) -> Result<Value> {
    for _ in 0..45 {
        let result = rpc_call(session, "contract_receipt", json!([tx_hash]));
        if let Ok(receipt) = result {
            if !receipt.is_null() {
                return Ok(receipt);
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    bail!("timed out waiting for receipt {tx_hash}")
}

fn wait_for_transaction(session: &Session, tx_hash: &str) -> Result<Value> {
    for _ in 0..60 {
        let result = rpc_call(session, "octra_transaction", json!([tx_hash]));
        if let Ok(transaction) = result {
            let status = transaction
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match status {
                "confirmed" | "accepted" => return Ok(transaction),
                "rejected" | "failed" => bail!("transaction {tx_hash} {status}: {transaction}"),
                _ => {}
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    bail!("timed out waiting for transaction {tx_hash}")
}

fn decode_rpc_result(result: Value) -> Result<Value> {
    if let Some(text) = result.get("result").and_then(Value::as_str) {
        return decode_method_result(text);
    }
    if let Some(text) = result.as_str() {
        return decode_method_result(text);
    }
    Ok(result)
}

fn decode_method_result(text: &str) -> Result<Value> {
    if let Some(encoded) = text.strip_prefix(TYPED_PREFIX) {
        return decode_typed_result(encoded);
    }
    Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string())))
}

fn is_read_sql(sql: &str) -> bool {
    let lower = sql.trim_start().to_ascii_lowercase();
    lower.starts_with("select") || lower.starts_with("with") || lower.starts_with("explain")
}

fn verify(session: &Session, expected_hash: Option<&str>, write_smoke: bool) -> Result<()> {
    println!("database: {}", session.target.raw);
    println!("circle: {}", session.target.circle);
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
    println!("program: version {version}, bytes {bytes}, hash {hash}");
    if let Some(expected) = expected_hash {
        if hash != expected {
            if expected == EXPECTED_WASM_SHA256 {
                match personalized_wasm_hash(session) {
                    Ok(Some(personalized_hash)) if hash == personalized_hash => {
                        println!("program: owner-personalized bundled WASM");
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
    }
    let storage = view(session, "storage_info", vec![])?;
    println!(
        "storage: {} pages, {} bytes, generation {}",
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
    );
    if let Ok(auth) = auth_info(session) {
        if auth.configured {
            println!(
                "auth: owner_write_intent owner={}, db_id={}, sequence={}",
                auth.owner_pubkey.as_deref().unwrap_or("?"),
                auth.db_id,
                auth.owner_sequence
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "?".to_string())
            );
        } else {
            println!("auth: unconfigured");
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
        let result = exec_sql(
            session,
            "create table if not exists octra_sqlite_verify(first_name text not null, last_name text not null);
delete from octra_sqlite_verify;
insert into octra_sqlite_verify(first_name,last_name) values ('Ava','North'),('Cora','Moss'),('Drew','Vale');",
            false,
        )?;
        print_exec_result(&result)?;
        let rows = query_typed(
            session,
            "select first_name,last_name from octra_sqlite_verify order by first_name;",
        )?;
        print_result(&rows, OutputMode::Table, true)?;
    }
    Ok(())
}

fn cmd_deploy(args: DeployArgs) -> Result<()> {
    let circle = args.circle.unwrap_or_else(|| DEFAULT_CIRCLE.to_string());
    let target_args = TargetArgs {
        target: Some(circle.clone()),
        wallet: args.wallet.clone(),
        rpc: Some(args.rpc.clone()),
        caller: args.caller.clone(),
        private_key_b64: args.private_key_b64.clone(),
        public_key_b64: args.public_key_b64.clone(),
    };
    let session = build_session(&target_args)?;
    let wasm_path = resolve_wasm_path(args.build, args.wasm.as_deref())?;
    let mut wasm =
        fs::read(&wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;
    let auth_patch = match auth_info(&session) {
        Ok(auth) if auth.configured => Some(patch_wasm_auth_from_info(&mut wasm, &auth).with_context(|| {
            "preserving existing owner write intent personalization; pass --allow-unconfigured to deploy raw WASM"
        })?),
        Ok(_) if args.allow_unconfigured => None,
        Ok(_) => bail!(
            "database Circle is not owner-write-intent personalized; refusing to deploy raw unsigned-write WASM without --allow-unconfigured"
        ),
        Err(error) if args.allow_unconfigured => {
            eprintln!("warning: auth_info unavailable; deploying unconfigured WASM because --allow-unconfigured was passed: {error:#}");
            None
        }
        Err(error) => bail!(
            "could not read database auth_info; refusing to deploy because it could remove owner-write protection: {error:#}. Pass --allow-unconfigured to deploy raw WASM."
        ),
    };
    let code_hash = sha256_hex(&wasm);
    let message = compact_json(&json!({
        "code_b64": general_purpose::STANDARD.encode(&wasm),
    }))?;
    let tx = Tx {
        from: session.caller.clone(),
        to_: session.target.circle.clone(),
        amount: "0".to_string(),
        nonce: next_nonce(&session)?,
        ou: args.ou,
        timestamp: now_timestamp(),
        op_type: "circle_program_update".to_string(),
        encrypted_data: String::new(),
        message,
        signature: String::new(),
        public_key: session.public_key_b64.clone(),
    };
    let result = submit_tx(&session, tx, true)?;
    let tx_hash = result
        .get("tx_hash")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut out = Map::new();
    out.insert(
        "circle".to_string(),
        Value::String(session.target.circle.clone()),
    );
    out.insert(
        "wasm".to_string(),
        Value::String(wasm_path.display().to_string()),
    );
    out.insert("code_bytes".to_string(), Value::Number(wasm.len().into()));
    out.insert("code_hash".to_string(), Value::String(code_hash.clone()));
    if let Some(patch) = auth_patch {
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
    print_json(&Value::Object(out))
}

fn redact_code_payload(value: Value) -> Value {
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

fn wait_for_program_info(session: &Session, expected_hash: &str) -> Result<Value> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let cli =
            Cli::try_parse_from(["octra-sqlite", "database", "info", "organization"]).unwrap();
        match cli.command {
            Commands::Database { command } => match command {
                DatabaseCommand::Info { database } => {
                    assert_eq!(database.as_deref(), Some("organization"));
                }
                _ => panic!("expected database info command"),
            },
            _ => panic!("expected database command"),
        }
    }

    #[test]
    fn status_and_config_are_public_commands() {
        let status = Cli::try_parse_from(["octra-sqlite", "status", "--skip-network"]).unwrap();
        match status.command {
            Commands::Status(args) => assert!(args.skip_network),
            _ => panic!("expected status command"),
        }

        let config = Cli::try_parse_from(["octra-sqlite", "config", "--json"]).unwrap();
        match config.command {
            Commands::Config(args) => assert!(args.json),
            _ => panic!("expected config command"),
        }
    }

    #[test]
    fn config_reads_legacy_names_and_writes_database_names() {
        let config: Config = serde_json::from_str(
            r#"{"default_target":"organization","aliases":{"organization":"oct://devnet/octABC"}}"#,
        )
        .unwrap();
        assert_eq!(config.default_database.as_deref(), Some("organization"));
        assert_eq!(
            config.databases.get("organization").map(String::as_str),
            Some("oct://devnet/octABC")
        );

        let written = serde_json::to_string(&config).unwrap();
        assert!(written.contains("default_database"));
        assert!(written.contains("databases"));
        assert!(!written.contains("default_target"));
        assert!(!written.contains("aliases"));
    }

    #[test]
    fn owner_write_intent_message_matches_golden_vector() {
        let digest = Sha256::digest(b"test-db-id");
        let mut db_id = [0u8; 32];
        db_id.copy_from_slice(&digest);
        let message = owner_write_intent_message(&db_id, 42, "exec", "select 1;").unwrap();
        assert_eq!(
            hex::encode(message),
            "6f637472612d73716c6974652e6f7377312e7631001fce55ad53f355909514a6a349e2afb2a22cf3bca124d239a9ace46a4108c482000000000000002a0004657865630000000973656c65637420313b"
        );
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
        ])
        .unwrap();
        match cli.command {
            Commands::New(args) => {
                assert_eq!(args.name, "my-db");
                assert_eq!(args.sql_args, vec!["create table people(first_name text);"]);
                assert!(args.wasm.is_none());
            }
            _ => panic!("expected new command"),
        }
    }

    #[test]
    fn new_accepts_builtin_sample() {
        let cli =
            Cli::try_parse_from(["octra-sqlite", "new", "my-db", "--sample", "people"]).unwrap();
        match cli.command {
            Commands::New(args) => {
                assert_eq!(args.name, "my-db");
                assert_eq!(args.sample.as_deref(), Some("people"));
            }
            _ => panic!("expected new command"),
        }
    }

    #[test]
    fn quickstart_defaults_to_people_sample() {
        let cli = Cli::try_parse_from(["octra-sqlite", "quickstart", "my-db"]).unwrap();
        match cli.command {
            Commands::Quickstart(args) => {
                assert_eq!(args.name, "my-db");
                assert_eq!(args.sample, "people");
                assert!(!args.no_default);
            }
            _ => panic!("expected quickstart command"),
        }
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
    fn doctor_accepts_local_only_mode() {
        let cli = Cli::try_parse_from(["octra-sqlite", "doctor", "--skip-network"]).unwrap();
        match cli.command {
            Commands::Doctor(args) => assert!(args.skip_network),
            _ => panic!("expected doctor command"),
        }
    }

    #[test]
    fn people_sample_creates_expected_table() {
        let sql = sample_sql("people").unwrap();
        assert!(sql.contains("create table people"));
        assert!(sql.contains("Ada"));
        assert!(sample_sql("unknown").is_err());
    }

    #[test]
    fn deploy_payload_json_matches_wasm_v1_circle_shape() {
        let payload = circle_deploy_payload_json(None).unwrap();
        assert_eq!(
            payload,
            "{\"runtime\":\"wasm_v1\",\"privacy_class\":\"sealed\",\"browser_mode\":\"native_sealed\",\"resource_mode\":\"sealed_read\",\"code_b64\":null,\"policy_hash\":null,\"members_root\":null,\"export_policy\":null,\"limits\":{\"max_stable_bytes\":\"33554432\",\"max_assets_bytes\":\"33554432\",\"max_inline_value\":\"65536\",\"max_wasm_bytes\":\"33554432\"}}"
        );
    }

    #[test]
    fn deploy_payload_json_can_inline_wasm_code() {
        let payload = circle_deploy_payload_json(Some("QUJD")).unwrap();
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

    #[test]
    fn base58_matches_known_vectors() {
        assert_eq!(base58_encode(&[0]), "1");
        assert_eq!(base58_encode(&[0, 0]), "11");
        assert_eq!(base58_encode(&[1]), "2");
        assert_eq!(base58_encode(b"hello world"), "StV1DL6CwTryKyV");
    }

    #[test]
    fn deploy_circle_canonical_tx_omits_encrypted_data() {
        let tx = Tx {
            from: "octA".into(),
            to_: "octB".into(),
            amount: "0".into(),
            nonce: 7,
            ou: "200000".into(),
            timestamp: 1.0,
            op_type: "deploy_circle".into(),
            encrypted_data: String::new(),
            message: circle_deploy_payload_json(Some("QUJD")).unwrap(),
            signature: String::new(),
            public_key: String::new(),
        };
        let canonical = canonical_tx(&tx);
        assert!(!canonical.contains("encrypted_data"));
        assert!(canonical.contains("\"op_type\":\"deploy_circle\""));
        assert!(canonical.contains("\\\"runtime\\\":\\\"wasm_v1\\\""));
    }

    #[test]
    fn deploy_circle_wire_tx_omits_empty_optional_fields() {
        let tx = Tx {
            from: "octA".into(),
            to_: "octB".into(),
            amount: "0".into(),
            nonce: 7,
            ou: "200000".into(),
            timestamp: 1.0,
            op_type: "deploy_circle".into(),
            encrypted_data: String::new(),
            message: circle_deploy_payload_json(Some("QUJD")).unwrap(),
            signature: "sig".into(),
            public_key: "pub".into(),
        };
        let wire = serde_json::to_value(tx).unwrap();
        assert!(wire.get("encrypted_data").is_none());
        assert!(wire.get("message").is_some());
    }

    #[test]
    fn canonical_tx_matches_field_order() {
        let tx = Tx {
            from: "octA".into(),
            to_: "octB".into(),
            amount: "0".into(),
            nonce: 7,
            ou: "1000".into(),
            timestamp: 1.0,
            op_type: "circle_call".into(),
            encrypted_data: "exec".into(),
            message: "[\"select 1;\"]".into(),
            signature: String::new(),
            public_key: String::new(),
        };
        assert_eq!(
            canonical_tx(&tx),
            "{\"from\":\"octA\",\"to_\":\"octB\",\"amount\":\"0\",\"nonce\":7,\"ou\":\"1000\",\"timestamp\":1.0,\"op_type\":\"circle_call\",\"encrypted_data\":\"exec\",\"message\":\"[\\\"select 1;\\\"]\"}"
        );
    }

    #[test]
    fn decodes_typed_result_cells() {
        let vector: Value =
            serde_json::from_str(include_str!("../tests/fixtures/osr1/basic.json")).unwrap();
        let encoded = vector["payload_b64"].as_str().unwrap();
        let decoded = decode_typed_result(encoded).unwrap();
        assert_eq!(decoded, vector["expected"]);
        assert_eq!(decoded["columns"][1], "integer");
        assert_eq!(decoded["rows"][0][0], Value::Null);
        assert_eq!(decoded["rows"][0][1], -7);
        assert_eq!(decoded["rows"][0][2], 1000.0);
        assert_eq!(decoded["rows"][0][3], "Ada");
        assert_eq!(decoded["rows"][0][4]["base64"], "QUI=");
    }
}
