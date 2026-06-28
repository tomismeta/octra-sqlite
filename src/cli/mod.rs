use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use clap::{Args, Parser, Subcommand};
mod output;
mod portability;
mod shell;
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
use portability::{
    backup_database, ensure_sql_text_fits, execute_sql_script, execute_sql_script_with_progress,
    plan_sql_script, run_local_sqlite_integrity, submit_sql_script_no_wait, SqlBatchProgress,
    SqlScriptExecution, SqlScriptPlan, MAX_SQL_TEXT_BYTES, SQL_BATCH_TARGET_BYTES,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use shell::{run_dot_command, run_shell};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_WASM_REL: &str = "circle/wasm/octra_sqlite_circle.wasm";
const BUILD_WASM_SCRIPT_REL: &str = "scripts/build-wasm.sh";
const RELEASE_MANIFEST_REL: &str = "release/octra-sqlite-0.3.1.json";
const OWNER_PUBKEY_PLACEHOLDER: &[u8; 32] = b"OSQL_OWNER_PUBKEY_V1_PLACEHOLDER";
const DB_ID_PLACEHOLDER: &[u8; 32] = b"OSQL_DATABASE_ID_V1_PLACEHOLDER0";
const EXPECTED_WASM_SHA256: &str =
    "39635962bffb470daced92396ee27e206e6b3ea000b4ec7a954d3bcd05ba662b";
const EXPECTED_WASM_BYTES: usize = 609_404;
const CREATE_ART_EXAMPLE: &str =
    "octra-sqlite new art \"create table artist(id integer primary key, name text not null);\"";

#[derive(Parser)]
#[command(name = "octra-sqlite", version)]
#[command(about = "Real SQLite inside an Octra Circle")]
#[command(after_long_help = "\
Examples:
  octra-sqlite setup
  octra-sqlite status
  octra-sqlite config
  octra-sqlite new art \"create table artist(id integer primary key, name text not null);\"
  octra-sqlite art \".tables\"
  octra-sqlite art \".backup main art.sqlite\"
  octra-sqlite art \".dump\" > art.sql
  octra-sqlite database list
  octra-sqlite database info art
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
    /// Restore SQL text into an existing database with chunked execution.
    Restore(RestoreArgs),
    /// Check SQL text for Octra SQLite script limits without writing.
    Check(CheckArgs),
    /// Show Octra SQLite limits and operational capabilities.
    Limits(LimitsArgs),
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
  octra-sqlite database info art
  octra-sqlite database use art
  octra-sqlite database set art oct://devnet/oct...
")]
enum DatabaseCommand {
    /// List saved database names.
    List {
        /// Print a stable JSON summary.
        #[arg(long)]
        json: bool,
    },
    /// Show the URI, network, Circle ID, and RPC for a database.
    Info {
        /// Database name, Circle ID, or oct:// database URI. Defaults to the current database.
        #[arg(value_name = "DATABASE")]
        database: Option<String>,
        /// Print a stable JSON summary.
        #[arg(long)]
        json: bool,
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
    /// Execute SQL from a file. Use - to read stdin.
    #[arg(long = "sql-file", value_name = "FILE")]
    sql_file: Option<PathBuf>,
    /// Refuse to submit state-changing SQL.
    #[arg(long)]
    read_only: bool,
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
    #[arg(long, alias = "sql-file")]
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
    #[arg(long, value_name = "NAME")]
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
    /// Print a stable JSON summary.
    #[arg(long)]
    json: bool,
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
    /// Print a stable JSON summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ConfigArgs {
    /// Print raw JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct RestoreArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// SQL dump/script to restore. Use - or omit to read stdin.
    #[arg(long = "file", alias = "sql-file", value_name = "FILE")]
    file: Option<PathBuf>,
    /// Print a stable JSON summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct CheckArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// SQL to check.
    #[arg(long)]
    sql: Option<String>,
    /// SQL file to check. Use - to read stdin.
    #[arg(long = "sql-file", alias = "file", value_name = "FILE")]
    sql_file: Option<PathBuf>,
    /// Print a stable JSON summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct LimitsArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// Print a stable JSON summary.
    #[arg(long)]
    json: bool,
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
        Commands::Restore(args) => cmd_restore(args),
        Commands::Check(args) => cmd_check(args),
        Commands::Limits(args) => cmd_limits(args),
        Commands::Verify(args) => {
            let session = build_session(&args.target)?;
            verify(
                &session,
                args.expected_hash.as_deref(),
                args.write_smoke,
                args.integrity,
                args.json,
            )
        }
        Commands::Status(args) => cmd_status(args, "status"),
        Commands::Config(args) => cmd_config(args),
        Commands::Deploy(args) => cmd_deploy(args),
        Commands::Install => {
            println!("cargo install --path . --locked");
            println!("octra-sqlite setup");
            println!("{CREATE_ART_EXAMPLE}");
            println!("octra-sqlite art \".tables\"");
            println!("octra-sqlite status art");
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
        "restore",
        "check",
        "limits",
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
        print_field("create", CREATE_ART_EXAMPLE);
        print_field(
            "example",
            "octra-sqlite quickstart my_collections --sample remilia",
        );
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
    let init_sql = collect_initializer_sql(&args)?;

    let mut config = load_config().unwrap_or_default();
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
    print_field("funding", funding_detail);
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

    if !args.no_name {
        if let Err(error) = save_new_database_alias(&args, &target_uri, &mut config) {
            print_circle_recovery(
                &args,
                &target_uri,
                "database alias was not saved after Circle creation",
                false,
            );
            return Err(error.context("database alias save failed after Circle creation"));
        }
        print_field("saved", "yes");
    } else {
        print_field("saved", "no");
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
        let session = match build_session(&session_args) {
            Ok(session) => session,
            Err(error) => {
                print_circle_recovery(
                    &args,
                    &target_uri,
                    "initializer session failed after Circle creation",
                    !args.no_name,
                );
                return Err(error.context("initializer session failed after Circle creation"));
            }
        };
        if let Err(error) = run_initializer_sql(&session, &args, &init_sql) {
            print_circle_recovery(
                &args,
                &target_uri,
                "initializer failed after Circle creation",
                !args.no_name,
            );
            return Err(error.context("initializer failed after Circle creation"));
        }
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

fn collect_initializer_sql(args: &NewArgs) -> Result<Vec<String>> {
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
    if init_sql.is_empty() {
        if let Some(sql) = read_stdin_sql()? {
            init_sql.push(sql);
        }
    }
    Ok(init_sql)
}

fn run_initializer_sql(session: &Session, args: &NewArgs, init_sql: &[String]) -> Result<()> {
    for sql in init_sql {
        if args.no_wait {
            let execution = submit_sql_script_no_wait(session, sql)?;
            for result in execution.results {
                let result = with_explorer(result, session);
                print_exec_result(&result)?;
            }
            print_field(
                "initializer",
                format!("{} statements submitted", execution.statements),
            );
        } else {
            let statements = execute_sql_script(session, sql)?;
            print_field("initializer", format!("{statements} statements"));
        }
    }
    Ok(())
}

fn save_new_database_alias(args: &NewArgs, target_uri: &str, config: &mut Config) -> Result<()> {
    config
        .databases
        .insert(args.name.clone(), target_uri.to_string());
    if args.default || config.default_database.is_none() {
        config.default_database = Some(args.name.clone());
    }
    write_config(config)?;
    Ok(())
}

fn print_circle_recovery(args: &NewArgs, target_uri: &str, problem: &str, saved: bool) {
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
                shell_quote(&args.name),
                shell_quote(target_uri)
            ),
        );
    }
    let followup_target = if saved {
        args.name.as_str()
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

fn shell_quote(value: &str) -> String {
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

fn dot_arg_quote(value: &str) -> String {
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

fn new_followup_target<'a>(name: &'a str, target_uri: &'a str, no_name: bool) -> &'a str {
    if no_name {
        target_uri
    } else {
        name
    }
}

pub(super) fn print_field(label: &str, detail: impl AsRef<str>) {
    print!("{}", format_field(label, detail));
}

fn cmd_status(args: StatusArgs, label: &str) -> Result<()> {
    let mut report = StatusReport::new(label, args.json);
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
        print_field("create", CREATE_ART_EXAMPLE);
    }
    Ok(())
}

struct StatusReport {
    label: String,
    json: bool,
    failures: usize,
    warnings: usize,
    items: Vec<Value>,
}

impl StatusReport {
    fn new(label: &str, json: bool) -> Self {
        Self {
            label: label.to_string(),
            json,
            failures: 0,
            warnings: 0,
            items: Vec::new(),
        }
    }

    fn ok(&mut self, label: &str, detail: impl AsRef<str>) {
        self.record("ok", label, detail);
    }

    fn warn(&mut self, label: &str, detail: impl AsRef<str>) {
        self.warnings += 1;
        self.record("warn", label, detail);
    }

    fn fail(&mut self, label: &str, detail: impl AsRef<str>) {
        self.failures += 1;
        self.record("fail", label, detail);
    }

    fn record(&mut self, status: &str, label: &str, detail: impl AsRef<str>) {
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

    fn finish(self, label: &str) -> Result<()> {
        if self.json {
            return print_json(&json!({
                "ok": self.failures == 0,
                "type": self.label,
                "schema": "octra-sqlite.cli.v1",
                "failures": self.failures,
                "warnings": self.warnings,
                "items": self.items,
            }));
        }
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

pub(super) fn linked_circle(network: &str, circle: &str) -> String {
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

fn print_database_list(config: &Config, json_mode: bool) -> Result<()> {
    if json_mode {
        let databases = config
            .databases
            .iter()
            .map(|(name, uri)| {
                json!({
                    "name": name,
                    "uri": uri,
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
        print_field("create", CREATE_ART_EXAMPLE);
        return Ok(());
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
    Ok(())
}

fn print_database_info(config: &Config, database: Option<&str>, json_mode: bool) -> Result<()> {
    let requested = database
        .map(str::to_string)
        .or_else(|| config.default_database.clone())
        .ok_or_else(|| anyhow!("no database supplied and no default database is configured"))?;
    let saved_uri = config.databases.get(&requested);
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
            }
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
    if let Some(path) = &args.sql_file {
        let sql = read_sql_file_arg(path)?;
        return run_sql_input(&session, &sql, mode, true, args.read_only);
    }
    if args.sql.is_empty() {
        if let Some(sql) = read_stdin_sql()? {
            return run_sql_input(&session, &sql, mode, true, args.read_only);
        }
        run_shell(session, mode)
    } else {
        let sql = args.sql.join(" ");
        run_sql_input(&session, &sql, mode, true, args.read_only)
    }
}

fn cmd_restore(args: RestoreArgs) -> Result<()> {
    let session = build_session(&args.target)?;
    let sql = match args.file.as_deref() {
        Some(path) => read_sql_file_arg(path)?,
        None => read_stdin_sql()?.ok_or_else(|| anyhow!("restore requires --file or piped SQL"))?,
    };
    let plan = plan_sql_script(&sql)?;
    if !args.json {
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
    }
    let mut progress_events = Vec::new();
    let mut execution = execute_sql_script_with_progress(&session, &sql, false, |progress| {
        if !args.json {
            print_field("restore", format_progress(&progress));
        }
        progress_events.push(progress);
    })?;
    for result in &mut execution.results {
        let raw = std::mem::take(result);
        *result = with_explorer(raw, &session);
    }
    if args.json {
        print_json(&restore_envelope(
            &session,
            &plan,
            &execution,
            &progress_events,
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

fn cmd_check(args: CheckArgs) -> Result<()> {
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

fn cmd_limits(args: LimitsArgs) -> Result<()> {
    let target = resolve_optional_target(&args.target)?;
    let limits = limits_json(target.clone());
    if args.json {
        return print_json(&limits);
    }
    print_field("max SQL bytes", MAX_SQL_TEXT_BYTES.to_string());
    print_field("batch target bytes", SQL_BATCH_TARGET_BYTES.to_string());
    print_field("transactions", "one accepted exec is atomic");
    print_field("user BEGIN/COMMIT", "unsupported across Octra writes");
    print_field(
        "restore",
        "chunked; multi-batch restore can partially apply",
    );
    print_field("writes", "OSW1 owner write intent");
    print_field("read-only", "client guard via --read-only");
    if let Some(target) = target {
        print_field("database", target["uri"].as_str().unwrap_or(""));
        print_field("network", target["network"].as_str().unwrap_or(""));
        print_field("circle", target["circle"].as_str().unwrap_or(""));
    }
    Ok(())
}

fn read_sql_file_arg(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let sql = read_stdin_sql()?.ok_or_else(|| anyhow!("stdin did not contain SQL"))?;
        return Ok(sql);
    }
    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

fn collect_check_sql(args: &CheckArgs) -> Result<String> {
    if let Some(path) = &args.sql_file {
        return read_sql_file_arg(path);
    }
    if let Some(sql) = &args.sql {
        return Ok(sql.clone());
    }
    read_stdin_sql()?.ok_or_else(|| anyhow!("check requires --sql, --sql-file, or piped SQL"))
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

fn run_sql_input(
    session: &Session,
    sql: &str,
    mode: OutputMode,
    headers: bool,
    read_only: bool,
) -> Result<()> {
    run_one_sql_to(session, sql, mode, headers, None, read_only)
}

pub(super) fn run_one_sql_to(
    session: &Session,
    sql: &str,
    mode: OutputMode,
    headers: bool,
    output: Option<&Path>,
    read_only: bool,
) -> Result<()> {
    let trimmed = sql.trim();
    if trimmed.starts_with('.') && !trimmed.contains('\n') {
        if read_only && write_dot_command(trimmed) {
            bail!("read_only: dot command may write to the database");
        }
        run_dot_command(session.clone(), mode, headers, output, trimmed)?;
        return Ok(());
    }
    if looks_like_sql_script(sql) {
        if read_only {
            bail!("read_only: multi-statement SQL scripts are not submitted in read-only mode");
        }
        return run_exec_script_to(session, sql, mode, output);
    }
    ensure_sql_text_fits(sql)?;
    match query_typed(session, sql) {
        Ok(result) => {
            if mode == OutputMode::Json {
                write_text(output, &format_json(&query_envelope(session, result))?)
            } else {
                write_text(output, &format_result(&result, mode, headers)?)
            }
        }
        Err(error) if sqlite_requires_exec(&error) => {
            if read_only {
                bail!("read_only: SQL would write; remove --read-only to sign and submit it");
            }
            run_exec_sql_to(session, sql, mode, output)
        }
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
        write_text(
            output,
            &format_json(&write_envelope(session, result, None))?,
        )
    } else {
        write_text(output, &format_exec_result(&result)?)
    }
}

fn run_exec_script_to(
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

fn write_dot_command(command: &str) -> bool {
    let name = command
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(name.as_str(), ".read" | ".import")
}

fn format_progress(progress: &SqlBatchProgress) -> String {
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

fn query_envelope(session: &Session, result: Value) -> Value {
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

fn write_envelope(session: &Session, result: Value, statements: Option<usize>) -> Value {
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

fn restore_envelope(
    session: &Session,
    plan: &SqlScriptPlan,
    execution: &SqlScriptExecution,
    progress: &[SqlBatchProgress],
) -> Value {
    script_envelope("restore", session, plan, execution, progress)
}

fn script_envelope(
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

fn progress_json(progress: &SqlBatchProgress) -> Value {
    json!({
        "batch_index": progress.batch_index,
        "total_batches": progress.total_batches,
        "start_statement": progress.start_statement,
        "end_statement": progress.end_statement,
        "statements": progress.statements,
        "bytes": progress.bytes,
    })
}

fn write_result_summary(result: &Value) -> Value {
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

fn script_plan_json(plan: &SqlScriptPlan) -> Value {
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

fn script_plan_warnings(plan: &SqlScriptPlan) -> Vec<String> {
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

fn database_identity(session: &Session) -> Value {
    let target = session.target();
    json!({
        "uri": canonical_database_uri(target),
        "raw": &target.raw,
        "network": &target.network,
        "circle": &target.circle,
        "rpc": &target.rpc,
        "wallet": session.caller(),
    })
}

fn canonical_database_uri(target: &Target) -> String {
    format!("oct://{}/{}", target.network, target.circle)
}

fn resolve_optional_target(args: &TargetArgs) -> Result<Option<Value>> {
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

fn limits_json(target: Option<Value>) -> Value {
    json!({
        "ok": true,
        "type": "limits",
        "schema": "octra-sqlite.cli.v1",
        "target": target,
        "sql": {
            "max_sql_bytes": MAX_SQL_TEXT_BYTES,
            "batch_target_bytes": SQL_BATCH_TARGET_BYTES,
            "input": ["argument", "stdin", "--sql-file", ".read", "restore"],
        },
        "transactions": {
            "exec_atomicity": "one accepted exec is atomic",
            "user_begin_commit": false,
            "multi_batch_atomic": false,
            "restore_partial_apply": true,
        },
        "auth": {
            "write_model": "OSW1 owner write intent",
            "read_only_guard": "client-side --read-only",
        }
    })
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

pub(super) fn verify(
    session: &Session,
    expected_hash: Option<&str>,
    write_smoke: bool,
    integrity: bool,
    json_mode: bool,
) -> Result<()> {
    if json_mode {
        return verify_json(session, expected_hash, write_smoke, integrity);
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

fn verify_json(
    session: &Session,
    expected_hash: Option<&str>,
    write_smoke: bool,
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
        let result = with_explorer(exec_sql(
            session,
            "create table if not exists octra_sqlite_verify(first_name text not null, last_name text not null);
delete from octra_sqlite_verify;
insert into octra_sqlite_verify(first_name,last_name) values ('Ava','North'),('Cora','Moss'),('Drew','Vale');",
            false,
        )?, session);
        Some(write_envelope(session, result, Some(3)))
    } else {
        None
    };
    let integrity_result = if integrity {
        let path = env::temp_dir().join(format!(
            "octra-sqlite-integrity-{}-{}.sqlite",
            session.target().circle,
            std::process::id()
        ));
        let summary = backup_database(session, &path)?;
        let result = run_local_sqlite_integrity(&path)?;
        let _ = fs::remove_file(&path);
        Some(json!({
            "result": result,
            "bytes": summary.bytes,
            "pages": summary.pages,
            "generation": summary.generation,
            "sha256": summary.sha256,
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
            "--json",
        ])
        .unwrap();
        match restore.command {
            Commands::Restore(args) => {
                assert_eq!(args.target.target.as_deref(), Some("art"));
                assert_eq!(args.file.as_deref(), Some(Path::new("dump.sql")));
                assert!(args.json);
            }
            _ => panic!("expected restore command"),
        }

        let check =
            Cli::try_parse_from(["octra-sqlite", "check", "art", "--sql-file", "-", "--json"])
                .unwrap();
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
        assert!(shell::import_args(&[
            "--csv".to_string(),
            "--skip".to_string(),
            "1".to_string(),
            "person.csv".to_string(),
            "person".to_string(),
        ])
        .is_ok());
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
            "insert into people values ('Ada');",
        ])
        .unwrap();
        match cli.command {
            Commands::New(args) => {
                assert_eq!(args.name, "my-db");
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
    fn quickstart_requires_explicit_sample() {
        assert!(Cli::try_parse_from(["octra-sqlite", "quickstart", "my-db"]).is_err());

        let cli =
            Cli::try_parse_from(["octra-sqlite", "quickstart", "my-db", "--sample", "remilia"])
                .unwrap();
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
