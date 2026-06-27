use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use clap::{Args, Parser, Subcommand};
mod output;
use crate::{
    client::{
        config_path, load_config,
        low_level::{
            auth_info, build_control_session as client_build_control_session,
            build_session as client_build_session, discover_wallet_path, exec_sql, next_nonce,
            program_info, query_typed, resolve_wallet_path as client_resolve_wallet_path,
            submit_tx, view, wait_for_transaction, wallet_caller, Session,
        },
        write_config, AuthInfo, ClientError, ClientErrorKind, Config, SessionOptions,
    },
    protocol::{
        target::{parse_database_target, DatabaseTarget as Target},
        tx::Tx,
    },
};
use output::{
    dim, format_exec_result, format_field, format_json, format_result, format_status_line,
    hyperlink, print_exec_result, print_json, print_result, strong, value_to_string, write_text,
    OutputMode,
};
use rustyline::error::ReadlineError;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_WASM_REL: &str = "circle/wasm/octra_sqlite_circle.wasm";
const BUILD_WASM_SCRIPT_REL: &str = "scripts/build-wasm.sh";
const RELEASE_MANIFEST_REL: &str = "release/octra-sqlite-0.3.0.json";
const OWNER_PUBKEY_PLACEHOLDER: &[u8; 32] = b"OSQL_OWNER_PUBKEY_V1_PLACEHOLDER";
const DB_ID_PLACEHOLDER: &[u8; 32] = b"OSQL_DATABASE_ID_V1_PLACEHOLDER0";
const EXPECTED_WASM_SHA256: &str =
    "8158f507a349cec2a97993d513ca2d3b275d9aaf4e39ea1edee414ce55d415ea";
const EXPECTED_WASM_BYTES: usize = 609_475;

#[derive(Parser)]
#[command(name = "octra-sqlite", version)]
#[command(about = "Real SQLite inside an Octra Circle")]
#[command(after_long_help = "\
Examples:
  octra-sqlite setup
  octra-sqlite status
  octra-sqlite config
  octra-sqlite new organization \"create table person(first_name text not null, last_name text not null);\"
  octra-sqlite organization \".tables\"
  octra-sqlite organization \".backup main organization.sqlite\"
  octra-sqlite organization \".dump\" > organization.sql
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
    /// Open a SQLite shell or run SQL against a database.
    Open(OpenArgs),
    /// Verify deployed database code, storage, typed queries, schema, and optionally a write.
    Verify(VerifyArgs),
    /// Show local config, wallet, bundled WASM, and live database health.
    Status(StatusArgs),
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
    /// Show the URI, network, Circle ID, and RPC for a database.
    Info {
        /// Database name, Circle ID, or oct:// database URI. Defaults to the current database.
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
    /// Database name, Circle ID, or oct:// database URI.
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
    #[arg(long)]
    rpc: Option<String>,
    /// Octra network name.
    #[arg(long)]
    network: Option<String>,
    /// Default database name, Circle ID, or oct:// database URI.
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
    /// Default database name, Circle ID, or oct:// database URI.
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
    #[arg(long)]
    rpc: Option<String>,
    /// Octra network name.
    #[arg(long)]
    network: Option<String>,
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
    #[arg(long, default_value = "remilia")]
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
    /// Circle ID to update.
    #[arg(long)]
    circle: Option<String>,
    /// Custom WASM program to deploy.
    #[arg(long)]
    wasm: Option<PathBuf>,
    /// OU budget for Circle program update.
    #[arg(long, default_value = "200000")]
    ou: String,
    /// Octra RPC URL.
    #[arg(long)]
    rpc: Option<String>,
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
    /// Back up to a temporary SQLite file and run local sqlite3 integrity_check.
    #[arg(long)]
    integrity: bool,
}

#[derive(Args)]
struct StatusArgs {
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

struct ShellState {
    session: Session,
    mode: OutputMode,
    headers: bool,
    timer: bool,
    output: Option<PathBuf>,
    once_output: Option<PathBuf>,
}

struct BackupSummary {
    path: PathBuf,
    bytes: u64,
    pages: u64,
    generation: u64,
    sha256: String,
}

pub fn run() -> Result<()> {
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
        Commands::Database { command } => cmd_database(command),
        Commands::Open(args) => cmd_open(args),
        Commands::Verify(args) => {
            let session = build_session(&args.target)?;
            verify(
                &session,
                args.expected_hash.as_deref(),
                args.write_smoke,
                args.integrity,
            )
        }
        Commands::Status(args) => cmd_status(args, "status"),
        Commands::Config(args) => cmd_config(args),
        Commands::Deploy(args) => cmd_deploy(args),
        Commands::Install => {
            println!("cargo install --path . --locked");
            println!("octra-sqlite setup");
            println!("octra-sqlite new organization \"create table person(first_name text not null, last_name text not null);\"");
            println!("octra-sqlite organization \".tables\"");
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
        "open",
        "verify",
        "status",
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

fn cmd_init(args: InitArgs) -> Result<()> {
    let mut config = load_config().unwrap_or_default();
    config.wallet = args
        .wallet
        .map(|p| p.to_string_lossy().to_string())
        .or(config.wallet);
    if let Some(network) = args.network {
        config.network = Some(network);
        config.apply_active_network_profile();
    }
    if let Some(rpc) = args.rpc {
        config.rpc = Some(rpc);
    }
    if let Some(database) = args.database {
        config.default_database = Some(database);
    }
    write_config(&config)?;
    print_field("wrote", config_path()?.display().to_string());
    if let Some(default_database) = &config.default_database {
        print_field("default database", default_database);
    }
    if let Some(network) = &config.network {
        print_field("network", network);
    }
    if let Some(rpc) = &config.rpc {
        print_field("rpc", rpc);
    }
    if let Some(explorer) = &config.explorer {
        print_field("explorer", explorer);
    }
    if let Some(wallet) = &config.wallet {
        print_field("wallet", wallet);
    }
    Ok(())
}

fn cmd_setup(args: SetupArgs) -> Result<()> {
    let mut config = load_config().unwrap_or_default();
    let interactive = !args.yes && io::stdin().is_terminal();
    if !interactive && !args.yes {
        bail!("setup is interactive; run it in a terminal, pass --yes, or use init with flags");
    }

    println!("{}", strong("Octra SQLite setup"));
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
            "{} wallet not found at {}; writes need a funded wallet",
            strong("warning:"),
            wallet_path.display()
        );
    }

    let network_default = args
        .network
        .clone()
        .or_else(|| config.network.clone())
        .ok_or_else(|| anyhow!("network is required; pass --network"))?;
    let network = if interactive {
        prompt_default("Network", &network_default)?
    } else {
        network_default
    };

    let rpc_default = args
        .rpc
        .clone()
        .or_else(|| env::var("OCTRA_RPC_URL").ok())
        .or_else(|| config.rpc_for_network(&network))
        .or_else(|| config.rpc.clone())
        .ok_or_else(|| anyhow!("RPC is required; pass --rpc or set OCTRA_RPC_URL"))?;
    let rpc = if interactive {
        prompt_default("RPC", &rpc_default)?
    } else {
        rpc_default
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
    config.network = Some(network.clone());
    config.apply_active_network_profile();
    config.rpc = Some(rpc.clone());
    if let Some(database) = database.filter(|value| !value.trim().is_empty()) {
        config.default_database = Some(database);
    }
    write_config(&config)?;
    print_field("wrote", config_path()?.display().to_string());
    print_field("wallet", wallet_path.display().to_string());
    print_field("network", &network);
    print_field("rpc", &rpc);
    if let Some(explorer) = config.explorer_for_network(&network) {
        print_field("explorer", explorer);
    }
    if let Some(default_database) = &config.default_database {
        print_field("default database", default_database);
    }

    if interactive && prompt_yes_no("Create a sample database now?", false)? {
        let name = prompt_default("Database name", "my_collections")?;
        let sample = prompt_default("Sample", "remilia")?;
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
        print_field(
            "create",
            "octra-sqlite new organization \"create table person(first_name text not null, last_name text not null);\"",
        );
        print_field("example", "octra-sqlite quickstart my_collections");
    }
    Ok(())
}

fn cmd_quickstart(args: QuickstartArgs) -> Result<()> {
    sample_sql(&args.sample)?;
    let config = load_config().unwrap_or_default();
    let network = args.network.clone().or_else(|| config.network.clone());
    let rpc = args.rpc.clone().or_else(|| {
        network
            .as_deref()
            .and_then(|network| config.rpc_for_network(network))
            .or_else(|| config.rpc.clone())
    });
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
    println!("  octra-sqlite {name} \"select name, launched_month from collection order by launched_month;\"");
    println!("  octra-sqlite {name}");
    Ok(())
}

fn cmd_new(args: NewArgs) -> Result<()> {
    let config = load_config().unwrap_or_default();
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

    let created = create_circle(&control_session, &args, &network)?;
    let target_uri = format!("oct://{}/{}", network, created.circle);
    if args.no_name {
        print_field("database", "(not saved)");
    } else {
        print_field("database", &args.name);
    }
    print_field("uri", &target_uri);
    print_field("circle", linked_circle(&network, &created.circle));
    print_field("wallet", control_session.caller());
    print_field(
        "code",
        format!("{} bytes, hash {}", created.code_bytes, created.code_hash),
    );
    print_field("auth", "owner-only writes");
    if let Some(hash) = &created.tx_hash {
        print_field("create_tx", linked_tx(&network, hash));
        if let Some(url) = explorer_tx_url(&network, hash) {
            print_field("create_tx_url", url);
        }
        if let Some(confirmation) = &created.confirmation {
            print_field(
                "create_status",
                confirmation
                    .get("status")
                    .map(value_to_string)
                    .unwrap_or_else(|| "unknown".to_string()),
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
            rpc: Some(control_session.rpc().to_string()),
            caller: args.caller.clone(),
            private_key_b64: args.private_key_b64.clone(),
            public_key_b64: args.public_key_b64.clone(),
        };
        let session = build_session(&session_args)?;
        for sql in init_sql {
            if args.no_wait {
                let result = with_explorer(exec_sql(&session, &sql, true)?, &session);
                print_exec_result(&result)?;
            } else {
                let statements = execute_sql_script(&session, &sql)?;
                print_field("initializer", format!("{statements} statements"));
            }
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
        print_field("saved", "yes");
    } else {
        print_field("saved", "no");
    }

    let followup_target = new_followup_target(&args.name, &target_uri, args.no_name);
    if args.no_name {
        print_field("open", format!("octra-sqlite open {target_uri}"));
    } else {
        print_field("open", format!("octra-sqlite open {}", args.name));
    }
    print_field("status", format!("octra-sqlite status {followup_target}"));
    Ok(())
}

fn new_followup_target<'a>(name: &'a str, target_uri: &'a str, no_name: bool) -> &'a str {
    if no_name {
        target_uri
    } else {
        name
    }
}

fn print_field(label: &str, detail: impl AsRef<str>) {
    print!("{}", format_field(label, detail));
}

fn cmd_status(args: StatusArgs, label: &str) -> Result<()> {
    let mut report = StatusReport::default();
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
                    "not set; run octra-sqlite database use DATABASE or pass a database argument",
                );
            }

            let wallet_path = resolve_wallet_path(&args.target, &config);
            match wallet_caller(wallet_path.as_deref(), args.target.caller.as_deref()) {
                Ok(caller) => {
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
                    if let Some(caller) = caller {
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
            "explorer": config.explorer,
            "networks": config.networks,
            "default_database": config.default_database,
            "databases": config.databases,
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
        print_field("create", "octra-sqlite quickstart my_collections");
    }
    Ok(())
}

#[derive(Default)]
struct StatusReport {
    failures: usize,
}

impl StatusReport {
    fn ok(&mut self, label: &str, detail: impl AsRef<str>) {
        print!("{}", format_status_line("ok", label, detail));
    }

    fn warn(&mut self, label: &str, detail: impl AsRef<str>) {
        print!("{}", format_status_line("warn", label, detail));
    }

    fn fail(&mut self, label: &str, detail: impl AsRef<str>) {
        self.failures += 1;
        print!("{}", format_status_line("fail", label, detail));
    }

    fn finish(self, label: &str) -> Result<()> {
        if self.failures == 0 {
            println!("{} ready", dim(format!("{label}:")));
            Ok(())
        } else {
            bail!("{label} found {} issue(s)", self.failures)
        }
    }
}

fn check_bundled_wasm(report: &mut StatusReport) {
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

fn check_release_manifest(report: &mut StatusReport) {
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

fn check_live_target(report: &mut StatusReport, session: &Session, expected_hash: &str) {
    report.ok("rpc", session.rpc());
    if let Some(url) = explorer_circle_url(&session.target().network, &session.target().circle) {
        report.ok("explorer", url);
    }
    match program_info(session) {
        Ok(info) => {
            report.ok(
                "circle",
                linked_circle(&session.target().network, &session.target().circle),
            );
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
                report.ok("auth", "OSW1 owner write intent");
                if let Some(owner_pubkey) = auth.owner_pubkey.as_deref() {
                    report.ok("auth owner pubkey", owner_pubkey);
                    match session.intent_public_key() {
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
        Ok(result) => report.ok(
            "sqlite version",
            first_result_cell(&result).unwrap_or_else(|| value_to_string(&result)),
        ),
        Err(error) => report.fail("sqlite version", error.to_string()),
    }
}

fn first_result_cell(result: &Value) -> Option<String> {
    result
        .get("rows")?
        .as_array()?
        .first()?
        .as_array()?
        .first()
        .map(value_to_string)
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

fn create_circle(session: &Session, args: &NewArgs, network: &str) -> Result<CreatedCircle> {
    let wasm_path = resolve_wasm_for_new(args)?;
    let mut wasm =
        fs::read(&wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;
    patch_wasm_auth_for_owner(&mut wasm, session)?;
    let code_hash = sha256_hex(&wasm);
    let code_b64 = general_purpose::STANDARD.encode(&wasm);
    let payload_json = circle_deploy_payload_json(Some(&code_b64))?;
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
        public_key: session.public_key_b64().to_string(),
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
    });
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

fn with_explorer(mut result: Value, session: &Session) -> Value {
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
    {
        if let Some(url) = explorer_tx_url(&session.target().network, &tx_hash) {
            object.insert("tx_url".to_string(), Value::String(url));
        }
    }
    result
}

fn linked_circle(network: &str, circle: &str) -> String {
    match explorer_circle_url(network, circle) {
        Some(url) => hyperlink(circle, url),
        None => circle.to_string(),
    }
}

fn linked_tx(network: &str, hash: &str) -> String {
    match explorer_tx_url(network, hash) {
        Some(url) => hyperlink(hash, url),
        None => hash.to_string(),
    }
}

fn explorer_base_url(network: &str) -> Option<String> {
    load_config()
        .unwrap_or_default()
        .explorer_for_network(network)
        .map(|url| url.trim_end_matches('/').to_string())
}

fn explorer_tx_url(network: &str, hash: &str) -> Option<String> {
    Some(format!(
        "{}/tx.html?hash={hash}",
        explorer_base_url(network)?
    ))
}

fn explorer_circle_url(network: &str, circle: &str) -> Option<String> {
    Some(format!(
        "{}/address.html?addr={circle}",
        explorer_base_url(network)?
    ))
}

fn patch_wasm_auth_for_owner(wasm: &mut [u8], session: &Session) -> Result<AuthPatch> {
    let owner_pubkey = session.intent_public_key()?;
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
        bail!("auth_info reports unconfigured OSW1 auth");
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
    hasher.update(session.caller().as_bytes());
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
            print_field("database", format!("{name} -> {database}"));
            print_field("open", format!("octra-sqlite {name}"));
        }
        DatabaseCommand::Use { name } => {
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
            if config.default_database.as_deref() == Some(&name) {
                config.default_database = None;
            }
            write_config(&config)?;
            print_field("removed", name);
        }
    }
    Ok(())
}

fn print_database_list(config: &Config) {
    if config.databases.is_empty() {
        println!("{}", dim("no databases"));
        print_field("create", "octra-sqlite quickstart my_collections");
        return;
    }
    println!("{}  name  uri", dim("default"));
    println!("{}", dim("-------  ----  ---"));
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
    print_field("open", format!("octra-sqlite {}", requested));
    print_field("status", format!("octra-sqlite status {}", requested));
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
            once_output: None,
        };
        handle_dot_command(&mut state, trimmed)?;
        return Ok(());
    }
    if looks_like_sql_script(sql) {
        return run_exec_sql_to(session, sql, mode, output);
    }
    match query_typed(session, sql) {
        Ok(result) => write_text(output, &format_result(&result, mode, headers)?),
        Err(error) if sqlite_requires_exec(&error) => run_exec_sql_to(session, sql, mode, output),
        Err(error) => Err(error.into()),
    }
}

fn run_exec_sql_to(
    session: &Session,
    sql: &str,
    mode: OutputMode,
    output: Option<&Path>,
) -> Result<()> {
    let result = with_explorer(exec_sql(session, sql, false)?, session);
    if mode == OutputMode::Json {
        write_text(output, &format_json(&result)?)
    } else {
        write_text(output, &format_exec_result(&result)?)
    }
}

fn format_schema_result(result: &Value) -> Result<String> {
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

fn backup_database(session: &Session, path: &Path) -> Result<BackupSummary> {
    let storage = view(session, "storage_info", vec![])?;
    let generation = json_u64(&storage, "generation")?;
    let page_count = json_u64(&storage, "page_count")?;
    let file_bytes = json_u64(&storage, "file_bytes")?;
    if page_count == 0 || file_bytes == 0 {
        bail!("database has no SQLite pages to back up");
    }

    let tmp_path = backup_temp_path(path);
    if let Some(parent) = tmp_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file =
        fs::File::create(&tmp_path).with_context(|| format!("creating {}", tmp_path.display()))?;
    let mut page = 1u64;
    while page <= page_count {
        let remaining = page_count - page + 1;
        let chunk_pages = remaining.min(8);
        let chunk = view(
            session,
            "backup_chunk",
            vec![
                Value::String(generation.to_string()),
                Value::String(page.to_string()),
                Value::String(chunk_pages.to_string()),
            ],
        )?;
        ensure_backup_chunk_matches(
            &chunk,
            generation,
            page,
            chunk_pages,
            page_count,
            file_bytes,
        )?;
        let encoded = chunk
            .get("data_b64")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("backup chunk missing data_b64"))?;
        let bytes = general_purpose::STANDARD
            .decode(encoded)
            .context("decoding backup chunk")?;
        let expected_len = (chunk_pages as usize) * 4096;
        if bytes.len() != expected_len {
            bail!(
                "backup chunk returned {} bytes; expected {expected_len}",
                bytes.len()
            );
        }
        file.write_all(&bytes)?;
        page += chunk_pages;
    }
    file.set_len(file_bytes)?;
    file.flush()?;
    drop(file);
    fs::rename(&tmp_path, path)
        .with_context(|| format!("moving {} to {}", tmp_path.display(), path.display()))?;
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let file_hash = hex::encode(hasher.finalize());
    Ok(BackupSummary {
        path: path.to_path_buf(),
        bytes: file_bytes,
        pages: page_count,
        generation,
        sha256: file_hash,
    })
}

fn backup_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "octra-sqlite-backup.sqlite".to_string());
    path.with_file_name(format!(".{file_name}.tmp.{}", std::process::id()))
}

fn ensure_backup_chunk_matches(
    chunk: &Value,
    generation: u64,
    start_page: u64,
    chunk_pages: u64,
    page_count: u64,
    file_bytes: u64,
) -> Result<()> {
    if chunk.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("backup chunk failed: {chunk}");
    }
    for (key, expected) in [
        ("generation", generation),
        ("start_page", start_page),
        ("chunk_pages", chunk_pages),
        ("page_count", page_count),
        ("file_bytes", file_bytes),
    ] {
        let actual = json_u64(chunk, key)?;
        if actual != expected {
            bail!("backup chunk {key} changed from {expected} to {actual}");
        }
    }
    Ok(())
}

fn json_u64(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("missing numeric {key}"))
}

fn run_local_sqlite_integrity(path: &Path) -> Result<String> {
    let output = ProcessCommand::new("sqlite3")
        .arg(path)
        .arg("pragma integrity_check;")
        .output()
        .with_context(|| "running sqlite3 integrity_check")?;
    if !output.status.success() {
        bail!(
            "sqlite3 integrity_check failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout != "ok" {
        bail!("sqlite3 integrity_check returned {stdout}");
    }
    Ok(stdout)
}

fn dump_database(session: &Session, objects: &[String]) -> Result<String> {
    run_sqlite_snapshot_dot_command(session, ".dump", objects)
}

fn fullschema_database(session: &Session) -> Result<String> {
    run_sqlite_snapshot_dot_command(session, ".fullschema", &[])
}

fn run_sqlite_snapshot_dot_command(
    session: &Session,
    command: &str,
    objects: &[String],
) -> Result<String> {
    let path = sqlite_snapshot_temp_path(command);
    let _cleanup = TempPathCleanup(path.clone());
    backup_database(session, &path)?;
    let mut dot_command = command.to_string();
    for object in objects {
        dot_command.push(' ');
        dot_command.push_str(&sqlite_dot_argument(object)?);
    }
    let output = ProcessCommand::new("sqlite3")
        .arg(&path)
        .arg(&dot_command)
        .output()
        .with_context(|| "running sqlite3 against exported backup")?;
    if !output.status.success() {
        bail!(
            "sqlite3 {command} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn sqlite_snapshot_temp_path(label: &str) -> PathBuf {
    let clean_label = label
        .trim_start_matches('.')
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>();
    std::env::temp_dir().join(format!(
        "octra-sqlite-{clean_label}-{}-{}.sqlite",
        std::process::id(),
        now_millis()
    ))
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

struct TempPathCleanup(PathBuf);

impl Drop for TempPathCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn sqlite_dot_argument(value: &str) -> Result<String> {
    if value.contains(['\0', '\n', '\r', '\'']) {
        bail!("sqlite object names for .dump cannot contain quotes or control characters");
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(value.to_string())
    } else {
        Ok(format!("'{value}'"))
    }
}

fn sql_string_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn execute_sql_script(session: &Session, sql: &str) -> Result<usize> {
    let statements = split_sql_statements(sql);
    if statements.is_empty() {
        return Ok(0);
    }
    if sql.trim().len() < 7_500 {
        let script = sql_script_for_single_exec(&statements);
        if script.trim().is_empty() {
            return Ok(0);
        }
        exec_sql(session, &script, false)?;
        return Ok(statements
            .iter()
            .filter(|statement| !should_skip_import_wrapper(statement))
            .count());
    }
    let mut batch = String::new();
    let mut executed = 0usize;
    for statement in statements {
        if should_skip_import_wrapper(&statement) {
            continue;
        }
        let candidate_len = batch.len() + statement.len() + 1;
        if !batch.is_empty() && candidate_len >= 7_500 {
            exec_sql(session, &batch, false).context(
                "executing SQL script batch; large .read files are applied in batches on Octra",
            )?;
            batch.clear();
        }
        if statement.len() >= 7_500 {
            exec_sql(session, &statement, false).context(
                "executing SQL script statement; large .read files are applied in batches on Octra",
            )?;
            executed += 1;
            continue;
        }
        batch.push_str(&statement);
        if !batch.ends_with(';') {
            batch.push(';');
        }
        batch.push('\n');
        executed += 1;
    }
    if !batch.trim().is_empty() {
        exec_sql(session, &batch, false).context(
            "executing SQL script batch; large .read files are applied in batches on Octra",
        )?;
    }
    Ok(executed)
}

fn should_skip_import_wrapper(statement: &str) -> bool {
    if should_skip_foreign_keys_pragma(statement) {
        return true;
    }
    let trimmed = statement
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_ascii_lowercase();
    trimmed == "begin"
        || trimmed == "begin transaction"
        || trimmed == "commit"
        || trimmed == "end"
        || trimmed == "rollback"
}

fn should_skip_foreign_keys_pragma(statement: &str) -> bool {
    let trimmed = statement
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_ascii_lowercase();
    trimmed.starts_with("pragma foreign_keys")
}

fn sql_script_for_single_exec(statements: &[String]) -> String {
    let mut script = String::new();
    for statement in statements {
        if should_skip_import_wrapper(statement) {
            continue;
        }
        script.push_str(statement.trim());
        if !statement.trim_end().ends_with(';') {
            script.push(';');
        }
        script.push('\n');
    }
    script
}

fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut start = 0usize;
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
                let statement = sql[start..=index].trim();
                if !statement.is_empty() {
                    if create_trigger_statement(statement)
                        && !trigger_statement_complete_at_semicolon(statement)
                    {
                        continue;
                    }
                    statements.push(statement.to_string());
                }
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = sql[start..].trim();
    if !tail.is_empty() {
        statements.push(tail.to_string());
    }
    statements
}

fn create_trigger_statement(statement: &str) -> bool {
    let tokens = sql_keyword_tokens(statement);
    if tokens.first().map(String::as_str) != Some("create") {
        return false;
    }
    let mut index = 1usize;
    if tokens
        .get(index)
        .is_some_and(|token| token == "temp" || token == "temporary")
    {
        index += 1;
    }
    tokens.get(index).is_some_and(|token| token == "trigger")
}

fn trigger_statement_complete_at_semicolon(statement: &str) -> bool {
    sql_tail_tokens_since_last_semicolon(statement) == ["end"]
}

fn sql_keyword_tokens(sql: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    scan_sql_tokens(sql, |token| tokens.push(token.to_ascii_lowercase()));
    tokens
}

fn sql_tail_tokens_since_last_semicolon(sql: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut tail = Vec::new();
    scan_sql_tokens(sql, |token| {
        if token == ";" {
            tail = std::mem::take(&mut tokens);
        } else {
            tokens.push(token.to_ascii_lowercase());
        }
    });
    if !tokens.is_empty() {
        tokens
    } else {
        tail
    }
}

fn scan_sql_tokens(mut sql: &str, mut visit: impl FnMut(&str)) {
    while !sql.is_empty() {
        let Some((offset, ch)) = sql.char_indices().next() else {
            break;
        };
        debug_assert_eq!(offset, 0);
        if ch.is_whitespace() {
            sql = &sql[ch.len_utf8()..];
            continue;
        }
        if sql.starts_with("--") {
            if let Some(end) = sql.find(['\n', '\r']) {
                sql = &sql[end..];
            } else {
                break;
            }
            continue;
        }
        if sql.starts_with("/*") {
            if let Some(end) = sql.find("*/") {
                sql = &sql[end + 2..];
            } else {
                break;
            }
            continue;
        }
        if ch == '\'' || ch == '"' || ch == '`' {
            sql = skip_quoted_sql(sql, ch);
            continue;
        }
        if ch == '[' {
            sql = skip_bracket_quoted_sql(sql);
            continue;
        }
        if ch == ';' {
            visit(";");
            sql = &sql[1..];
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let end = sql
                .char_indices()
                .find(|(_, token_ch)| !(token_ch.is_ascii_alphanumeric() || *token_ch == '_'))
                .map(|(idx, _)| idx)
                .unwrap_or(sql.len());
            visit(&sql[..end]);
            sql = &sql[end..];
            continue;
        }
        sql = &sql[ch.len_utf8()..];
    }
}

fn skip_quoted_sql(sql: &str, quote: char) -> &str {
    let mut chars = sql.char_indices();
    chars.next();
    while let Some((idx, ch)) = chars.next() {
        if ch == quote {
            if chars.clone().next().is_some_and(|(_, next)| next == quote) {
                chars.next();
            } else {
                let end = idx + ch.len_utf8();
                return &sql[end..];
            }
        }
    }
    ""
}

fn skip_bracket_quoted_sql(sql: &str) -> &str {
    let mut chars = sql.char_indices();
    chars.next();
    while let Some((idx, ch)) = chars.next() {
        if ch == ']' {
            if chars.clone().next().is_some_and(|(_, next)| next == ']') {
                chars.next();
            } else {
                return &sql[idx + 1..];
            }
        }
    }
    ""
}

fn import_csv(session: &Session, path: &Path, table: &str, skip: usize) -> Result<usize> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut inserted = 0usize;
    let mut batch = String::new();
    for (idx, row) in parse_csv_records(&text)?.into_iter().enumerate() {
        if idx < skip {
            continue;
        }
        let statement = format!(
            "insert into {} values({});",
            quote_identifier(table),
            row.iter()
                .map(|value| sql_string_literal(value))
                .collect::<Vec<_>>()
                .join(",")
        );
        if !batch.is_empty() && batch.len() + statement.len() + 1 >= 7_500 {
            exec_sql(session, &batch, false)?;
            batch.clear();
        }
        batch.push_str(&statement);
        batch.push('\n');
        inserted += 1;
    }
    if !batch.trim().is_empty() {
        exec_sql(session, &batch, false)?;
    }
    Ok(inserted)
}

fn parse_csv_records(text: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek().is_some_and(|next| *next == '"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => in_quotes = true,
            ',' => {
                row.push(std::mem::take(&mut field));
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\r' => {
                if chars.peek().is_some_and(|next| *next == '\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(ch),
        }
    }
    if in_quotes {
        bail!("unterminated CSV quote");
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

fn run_shell(session: Session, mode: OutputMode) -> Result<()> {
    println!(
        "{}",
        strong(format!("SQLite on Octra ({})", session.target().network))
    );
    print_field(
        "circle",
        linked_circle(&session.target().network, &session.target().circle),
    );
    print_field("wallet", session.caller());
    println!("{}", dim("type .help for usage"));
    let mut state = ShellState {
        session,
        mode,
        headers: true,
        timer: false,
        output: None,
        once_output: None,
    };
    let mut editor = rustyline::DefaultEditor::new()?;
    let history_path = shell_history_path()?;
    let _ = editor.load_history(&history_path);
    let mut buffer = String::new();
    loop {
        let prompt = if buffer.trim().is_empty() {
            "sqlite> "
        } else {
            "   ...> "
        };
        let line = match editor.readline(prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => break,
            Err(error) => return Err(error.into()),
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let _ = editor.add_history_entry(line.as_str());
        if buffer.trim().is_empty() && trimmed.starts_with('.') {
            if handle_dot_command(&mut state, trimmed)? {
                break;
            }
            continue;
        }
        buffer.push_str(&line);
        buffer.push('\n');
        if trimmed.ends_with(';') {
            let sql = buffer.trim().to_string();
            buffer.clear();
            let started = Instant::now();
            let output = take_command_output(&mut state);
            if let Err(error) = run_one_sql_to(
                &state.session,
                &sql,
                state.mode,
                state.headers,
                output.as_deref(),
            ) {
                eprintln!("error: {error:#}");
            }
            if state.timer {
                println!("Run Time: real {:.3}", started.elapsed().as_secs_f64());
            }
        }
    }
    let _ = editor.save_history(&history_path);
    Ok(())
}

fn shell_history_path() -> Result<PathBuf> {
    let path = config_path()?.with_file_name("sqlite_history");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(path)
}

fn handle_dot_command(state: &mut ShellState, line: &str) -> Result<bool> {
    let parts = parse_dot_parts(line)?;
    let cmd = parts.first().map(String::as_str).unwrap_or("");
    let args = &parts[1..];
    match cmd {
        ".quit" | ".exit" => return Ok(true),
        ".help" => print_help(),
        ".mode" => {
            let mode = args
                .first()
                .map(String::as_str)
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
            let value = args
                .first()
                .map(String::as_str)
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
            let output = take_command_output(state);
            write_text(
                output.as_deref(),
                &format_result(&result, state.mode, state.headers)?,
            )?;
        }
        ".schema" => {
            let sql = if let Some(name) = args.first() {
                format!(
                    "select type, name, sql from sqlite_master where sql is not null and (name = {} or tbl_name = {}) order by type, name;",
                    sql_string_literal(name),
                    sql_string_literal(name)
                )
            } else {
                "select type, name, sql from sqlite_master where sql is not null and type in ('table','view','index','trigger') order by type, name;".to_string()
            };
            let result = query_typed(&state.session, &sql)?;
            let output = take_command_output(state);
            write_text(output.as_deref(), &format_schema_result(&result)?)?;
        }
        ".show" => print_shell_show(state),
        ".databases" => print_current_database(&state.session),
        ".storage" => write_text(
            take_command_output(state).as_deref(),
            &format_json(&view(&state.session, "storage_info", vec![])?)?,
        )?,
        ".circle" => write_text(
            take_command_output(state).as_deref(),
            &format_json(&program_info(&state.session)?)?,
        )?,
        ".wallet" => {
            println!("{}", state.session.caller());
            if let Some(path) = state.session.wallet_path() {
                print_field("wallet", path.display().to_string());
            }
        }
        ".verify" => verify(&state.session, None, false, false)?,
        ".backup" => {
            let path = backup_path_from_args(args)?;
            let summary = backup_database(&state.session, &path)?;
            print_backup_summary(&summary);
        }
        ".save" => {
            let path = save_path_from_args(args)?;
            let summary = backup_database(&state.session, &path)?;
            print_backup_summary(&summary);
        }
        ".dump" => {
            let output = take_command_output(state);
            write_text(output.as_deref(), &dump_database(&state.session, args)?)?;
        }
        ".read" => {
            let path = args.first().ok_or_else(|| anyhow!("usage: .read FILE"))?;
            reject_shell_pipe_arg(path, ".read")?;
            let sql = fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
            execute_sql_script(&state.session, &sql)?;
        }
        ".import" => {
            let (path, table, skip) = import_args(args)?;
            let inserted = import_csv(&state.session, &path, &table, skip)?;
            print_field("imported", format!("{inserted} rows"));
        }
        ".indexes" => {
            let sql = if let Some(table) = args.first() {
                format!(
                    "select name from sqlite_master where type='index' and tbl_name = {} order by name;",
                    sql_string_literal(table)
                )
            } else {
                "select name from sqlite_master where type='index' order by name;".to_string()
            };
            let result = query_typed(&state.session, &sql)?;
            let output = take_command_output(state);
            write_text(
                output.as_deref(),
                &format_result(&result, state.mode, state.headers)?,
            )?;
        }
        ".fullschema" => {
            let output = take_command_output(state);
            write_text(output.as_deref(), &fullschema_database(&state.session)?)?;
        }
        ".timer" => {
            let value = args
                .first()
                .map(String::as_str)
                .ok_or_else(|| anyhow!("usage: .timer on|off"))?;
            state.timer = match value {
                "on" => true,
                "off" => false,
                _ => bail!("usage: .timer on|off"),
            };
        }
        ".output" => {
            let Some(value) = args.first() else {
                state.output = None;
                return Ok(false);
            };
            reject_shell_pipe_arg(value, ".output")?;
            if value == "stdout" || value == "off" {
                state.output = None;
            } else {
                fs::write(value, "").with_context(|| format!("opening output {value}"))?;
                state.output = Some(PathBuf::from(value));
            }
        }
        ".once" => {
            let value = args.first().ok_or_else(|| anyhow!("usage: .once FILE"))?;
            if value.starts_with('-') {
                bail!(".once options are not supported");
            }
            reject_shell_pipe_arg(value, ".once")?;
            fs::write(value, "").with_context(|| format!("opening output {value}"))?;
            state.once_output = Some(PathBuf::from(value));
        }
        ".open" => {
            let target = args
                .first()
                .map(String::as_str)
                .ok_or_else(|| anyhow!("usage: .open DATABASE"))?;
            state.session = state.session.open_database(target)?;
            print_field(
                "circle",
                linked_circle(
                    &state.session.target().network,
                    &state.session.target().circle,
                ),
            );
        }
        _ => bail!("unknown command {cmd}; try .help"),
    }
    Ok(false)
}

fn print_help() {
    println!("SQLite commands:");
    println!("  .backup ?DB? FILE    back up main database to a SQLite file");
    println!("  .save FILE           save main database to a SQLite file");
    println!("  .dump ?OBJECTS?      render SQL text for the database or table");
    println!("  .import --csv FILE TABLE");
    println!("                      import CSV rows into TABLE");
    println!("  .indexes ?TABLE?     list indexes");
    println!("  .fullschema          show SQLite .fullschema output");
    println!("  .tables              list tables");
    println!("  .schema ?TABLE?      show schema");
    println!("  .databases           show the current main database URI");
    println!("  .open DATABASE       switch database name, Circle ID, or oct:// URI");
    println!("  .read FILE           execute SQL from a file");
    println!("  .mode MODE           MODE is box, table, list, json, line, or csv");
    println!("  .headers on|off      show or hide column headers");
    println!("  .timer on|off        show query timing");
    println!("  .output ?FILE?       redirect output; no FILE restores stdout");
    println!("  .once FILE           redirect the next command only");
    println!("  .show                show shell settings");
    println!("  .quit                exit");
    println!();
    println!("Octra commands:");
    println!("  .storage             show SQLite page storage info");
    println!("  .circle              show Circle program metadata");
    println!("  .wallet              show active wallet address");
    println!("  .verify              verify live Circle SQLite status");
}

fn parse_dot_parts(line: &str) -> Result<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = line.trim().chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(ch) = chars.next() {
        if let Some(active) = quote {
            if ch == active {
                if chars.peek().is_some_and(|next| *next == active) {
                    chars.next();
                    current.push(active);
                } else {
                    quote = None;
                }
            } else {
                current.push(ch);
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            ch if ch.is_whitespace() => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if quote.is_some() {
        bail!("unterminated quoted dot-command argument");
    }
    if !current.is_empty() {
        parts.push(current);
    }
    Ok(parts)
}

fn take_command_output(state: &mut ShellState) -> Option<PathBuf> {
    state.once_output.take().or_else(|| state.output.clone())
}

fn backup_path_from_args(args: &[String]) -> Result<PathBuf> {
    match args {
        [file] => Ok(PathBuf::from(file)),
        [db, file] if db == "main" => Ok(PathBuf::from(file)),
        [db, _] => bail!("only .backup main FILE is supported; got database {db}"),
        _ => bail!("usage: .backup ?main? FILE"),
    }
}

fn save_path_from_args(args: &[String]) -> Result<PathBuf> {
    match args {
        [file] => Ok(PathBuf::from(file)),
        [option, _] if option.starts_with('-') => bail!(".save options are not supported"),
        _ => bail!("usage: .save FILE"),
    }
}

fn reject_shell_pipe_arg(value: &str, command: &str) -> Result<()> {
    if value.starts_with('|') {
        bail!("{command} shell pipes are intentionally unsupported");
    }
    Ok(())
}

fn import_args(args: &[String]) -> Result<(PathBuf, String, usize)> {
    let mut csv = false;
    let mut skip = 0usize;
    let mut positional = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--csv" => {
                csv = true;
                index += 1;
            }
            "--skip" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("usage: .import --csv --skip N FILE TABLE"))?;
                skip = value.parse::<usize>().context("parsing .import --skip")?;
                index += 2;
            }
            value if value.starts_with('-') => bail!("unsupported .import option {value}"),
            value => {
                positional.push(value.to_string());
                index += 1;
            }
        }
    }
    if !csv {
        bail!("only .import --csv FILE TABLE is supported");
    }
    if positional.len() != 2 {
        bail!("usage: .import --csv [--skip N] FILE TABLE");
    }
    reject_shell_pipe_arg(&positional[0], ".import")?;
    Ok((PathBuf::from(&positional[0]), positional[1].clone(), skip))
}

fn print_backup_summary(summary: &BackupSummary) {
    print_field("backup", summary.path.display().to_string());
    print_field(
        "database",
        format!(
            "{} bytes, {} pages, generation {}",
            summary.bytes, summary.pages, summary.generation
        ),
    );
    print_field("sha256", &summary.sha256);
}

fn print_current_database(session: &Session) {
    println!("seq  name  file");
    println!("{}", dim("---  ----  ----"));
    println!("0    main  {}", session.target().raw);
}

fn print_shell_show(state: &ShellState) {
    print_field("database", &state.session.target().raw);
    print_field(
        "circle",
        linked_circle(
            &state.session.target().network,
            &state.session.target().circle,
        ),
    );
    print_field("network", &state.session.target().network);
    print_field("rpc", state.session.rpc());
    print_field("wallet", state.session.caller());
    print_field("mode", state.mode.name());
    print_field("headers", if state.headers { "on" } else { "off" });
    print_field("timer", if state.timer { "on" } else { "off" });
    print_field(
        "output",
        state
            .output
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "stdout".to_string()),
    );
}

fn session_options(args: &TargetArgs) -> SessionOptions {
    SessionOptions {
        target: args.target.clone(),
        wallet: args.wallet.clone(),
        rpc: args.rpc.clone(),
        caller: args.caller.clone(),
        private_key: args.private_key_b64.clone(),
        public_key: args.public_key_b64.clone(),
    }
}

fn resolve_wallet_path(args: &TargetArgs, config: &Config) -> Option<PathBuf> {
    client_resolve_wallet_path(&session_options(args), config)
}

fn build_session(args: &TargetArgs) -> Result<Session> {
    Ok(client_build_session(&session_options(args))?)
}

fn build_control_session(args: &TargetArgs, network: &str) -> Result<Session> {
    Ok(client_build_control_session(
        &session_options(args),
        network,
    )?)
}

fn sample_sql(name: &str) -> Result<String> {
    match name {
        "remilia" => Ok(include_str!("../../examples/remilia-collections.sql").to_string()),
        _ => bail!("unknown sample {name}; available samples: remilia"),
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
    let mut target = parse_database_target(value, config.network.as_deref(), None)?;
    if target.rpc.is_empty() {
        target.rpc = config.rpc_for_network(&target.network).unwrap_or_default();
    }
    Ok(target)
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn now_timestamp() -> f64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_secs() as f64 + f64::from(duration.subsec_millis()) / 1000.0
}

fn sqlite_requires_exec(error: &ClientError) -> bool {
    error.kind() == ClientErrorKind::Rpc
        && error
            .to_string()
            .starts_with("database error (sqlite_readonly_required)")
}

fn looks_like_sql_script(sql: &str) -> bool {
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

fn sql_tail_has_statement(mut tail: &str) -> bool {
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

fn verify(
    session: &Session,
    expected_hash: Option<&str>,
    write_smoke: bool,
    integrity: bool,
) -> Result<()> {
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
    if let Some(expected) = expected_hash {
        if hash != expected {
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
        let result = with_explorer(exec_sql(
            session,
            "create table if not exists octra_sqlite_verify(first_name text not null, last_name text not null);
delete from octra_sqlite_verify;
insert into octra_sqlite_verify(first_name,last_name) values ('Ava','North'),('Cora','Moss'),('Drew','Vale');",
            false,
        )?, session);
        print_exec_result(&result)?;
        let rows = query_typed(
            session,
            "select first_name,last_name from octra_sqlite_verify order by first_name;",
        )?;
        print_result(&rows, OutputMode::Table, true)?;
    }
    if integrity {
        let path = env::temp_dir().join(format!(
            "octra-sqlite-integrity-{}-{}.sqlite",
            session.target().circle,
            std::process::id()
        ));
        let summary = backup_database(session, &path)?;
        let result = run_local_sqlite_integrity(&path)?;
        let _ = fs::remove_file(&path);
        print_field(
            "integrity",
            format!(
                "{result}; checked {} bytes from generation {}",
                summary.bytes, summary.generation
            ),
        );
    }
    Ok(())
}

fn cmd_deploy(args: DeployArgs) -> Result<()> {
    let circle = args
        .circle
        .clone()
        .ok_or_else(|| anyhow!("deploy requires --circle CIRCLE_ID"))?;
    let target_args = TargetArgs {
        target: Some(circle.clone()),
        wallet: args.wallet.clone(),
        rpc: args.rpc.clone(),
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
        public_key: session.public_key_b64().to_string(),
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
    fn sqlite_readonly_required_routes_to_signed_exec() {
        let error = ClientError::with_kind(
            ClientErrorKind::Rpc,
            "database error (sqlite_readonly_required): use exec for state-changing SQL",
        );
        assert!(sqlite_requires_exec(&error));

        let error = ClientError::with_kind(
            ClientErrorKind::Rpc,
            "database error (sqlite_prepare_failed): no such table: missing",
        );
        assert!(!sqlite_requires_exec(&error));

        let error = ClientError::with_kind(
            ClientErrorKind::Rpc,
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
        let statements = split_sql_statements(
            "insert into t values ('semi;colon'); -- ; comment\ninsert into t values (\"two;semi\");",
        );
        assert_eq!(statements.len(), 2);
        assert!(statements[0].contains("'semi;colon'"));
        assert!(statements[1].contains("\"two;semi\""));
    }

    #[test]
    fn sql_script_splitter_keeps_triggers_whole() {
        let statements = split_sql_statements(
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
    fn sqlite_dump_wrappers_are_skipped_for_octra_restore() {
        assert!(should_skip_import_wrapper("PRAGMA foreign_keys=OFF;"));
        assert!(should_skip_import_wrapper("BEGIN TRANSACTION;"));
        assert!(should_skip_import_wrapper("COMMIT;"));
        assert!(!should_skip_import_wrapper(
            "create table person(id integer);"
        ));
    }

    #[test]
    fn small_sqlite_dump_restore_skips_shell_wrappers() {
        let statements = split_sql_statements(
            "PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE person(id integer);
COMMIT;",
        );
        let script = sql_script_for_single_exec(&statements);
        assert!(!script.contains("foreign_keys"));
        assert!(!script.contains("BEGIN TRANSACTION"));
        assert!(script.contains("CREATE TABLE person"));
        assert!(!script.contains("COMMIT"));
    }

    #[test]
    fn dot_parser_handles_quotes_and_rejects_shell_pipe_forms() {
        assert_eq!(
            parse_dot_parts(".backup main \"organization copy.sqlite\"").unwrap(),
            vec![".backup", "main", "organization copy.sqlite"]
        );
        assert!(reject_shell_pipe_arg("|cat", ".read").is_err());
        assert!(import_args(&[
            "--csv".to_string(),
            "--skip".to_string(),
            "1".to_string(),
            "person.csv".to_string(),
            "person".to_string(),
        ])
        .is_ok());
        assert!(import_args(&["person.csv".to_string(), "person".to_string()]).is_err());
    }

    #[test]
    fn sqlite_dot_arguments_are_quoted_without_shell_escape() {
        assert_eq!(sqlite_dot_argument("person").unwrap(), "person");
        assert_eq!(
            sqlite_dot_argument("person table").unwrap(),
            "'person table'"
        );
        assert_eq!(sqlite_dot_argument("person-table").unwrap(), "person-table");
        assert!(sqlite_dot_argument("person'table").is_err());
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
            wallet: None,
            caller: None,
            private_key_b64: None,
            public_key_b64: None,
        };
        let error = cmd_deploy(args).unwrap_err().to_string();
        assert!(error.contains("requires --circle"));
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
            Cli::try_parse_from(["octra-sqlite", "new", "my-db", "--sample", "remilia"]).unwrap();
        match cli.command {
            Commands::New(args) => {
                assert_eq!(args.name, "my-db");
                assert_eq!(args.sample.as_deref(), Some("remilia"));
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
    fn quickstart_defaults_to_remilia_sample() {
        let cli = Cli::try_parse_from(["octra-sqlite", "quickstart", "my-db"]).unwrap();
        match cli.command {
            Commands::Quickstart(args) => {
                assert_eq!(args.name, "my-db");
                assert_eq!(args.sample, "remilia");
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
    fn status_accepts_local_only_mode() {
        let cli = Cli::try_parse_from(["octra-sqlite", "status", "--skip-network"]).unwrap();
        match cli.command {
            Commands::Status(args) => assert!(args.skip_network),
            _ => panic!("expected status command"),
        }
    }

    #[test]
    fn remilia_sample_creates_expected_table() {
        let sql = sample_sql("remilia").unwrap();
        assert!(sql.contains("create table collection"));
        assert!(sql.contains("Milady Maker"));
        assert!(!sql.contains("source_url"));
        assert!(!sql.contains("notes"));
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
}
