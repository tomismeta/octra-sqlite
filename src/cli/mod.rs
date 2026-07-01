use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use clap::{Args, Parser, Subcommand, ValueEnum};
mod output;
mod portability;
mod shell;
use crate::{
    client::{
        config_path, load_config,
        low_level::{
            auth_info, build_control_session as client_build_control_session,
            build_session as client_build_session, circle_info, discover_wallet_path, exec_sql,
            next_nonce, program_info, query_typed, query_typed_traced,
            resolve_wallet_path as client_resolve_wallet_path, submit_tx, view,
            wait_for_transaction, wallet_caller, wallet_file_material,
            wallet_material_from_private_key, Session, WalletMaterial,
        },
        write_config, AuthInfo, ClientError, ClientErrorKind, Config, DatabaseMetadata,
        RpcTraceMode, SessionOptions,
    },
    protocol::{
        base58,
        target::{parse_database_target, DatabaseTarget as Target, ReadMode},
        tx::Tx,
    },
};
use output::{
    dim, format_exec_result, format_field, format_json, format_result, format_status_line,
    hyperlink, print_exec_result, print_json, print_result, strong, value_to_string, write_text,
    OutputMode,
};
use portability::{
    backup_database, ensure_sql_text_fits, execute_sql_script_with_bootstrap_owner_progress,
    execute_sql_script_with_owner_auth_progress, execute_sql_script_with_progress, plan_sql_script,
    run_local_sqlite_integrity, submit_sql_script_no_wait, SqlBatchProgress, SqlScriptExecution,
    SqlScriptPlan, MAX_SQL_TEXT_BYTES, SQL_BATCH_TARGET_BYTES,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use shell::{run_dot_command, run_shell};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

const DEFAULT_WASM_REL: &str = "circle/wasm/octra_sqlite_circle.wasm";
const BUILD_WASM_SCRIPT_REL: &str = "scripts/build-wasm.sh";
const RELEASE_MANIFEST_REL: &str = "release/octra-sqlite-0.4.0.json";
const OWNER_PUBKEY_PLACEHOLDER: &[u8; 32] = b"OSQL_OWNER_PUBKEY_V1_PLACEHOLDER";
const DB_ID_PLACEHOLDER: &[u8; 32] = b"OSQL_DATABASE_ID_V1_PLACEHOLDER0";
const EXPECTED_WASM_SHA256: &str =
    "36664d04fd0457c4c7da200328c753984746769cec479fd93f799665c66f8c5d";
const EXPECTED_WASM_BYTES: usize = 609_354;
const CREATE_DATABASE_COMMAND: &str = "octra-sqlite new";
const REPO_URL: &str = "https://github.com/tomismeta/octra-sqlite";
const MIN_RUST_VERSION: &str = "1.87";
const SQLITE_VERSION: &str = "3.53.2";
const MAX_RESULT_ROWS: usize = 512;
const MAX_RESPONSE_BYTES: usize = 65_526;

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
  octra-sqlite commands --json
")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive setup for wallet and network defaults.
    Setup(SetupArgs),
    /// Create a new SQLite database on Octra and optionally initialize it with SQL.
    New(NewArgs),
    /// Manage saved database names.
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
    /// Show supported CLI commands and JSON envelopes.
    #[command(name = "commands")]
    CommandList(CommandsArgs),
    /// Verify deployed database code, storage, typed queries, schema, and optionally a write.
    Verify(VerifyArgs),
    /// Show local config, wallet, bundled WASM, and live database health.
    Status(StatusArgs),
    /// Show local wallet, RPC, network, and database configuration.
    Config(ConfigArgs),
    /// Inspect wallet path, permissions, caller, and target read/write status.
    Wallet {
        #[command(subcommand)]
        command: WalletCommand,
    },
    /// Deploy/update a Circle program through native signed RPC.
    Deploy(DeployArgs),
    /// Print installation instructions for the Rust CLI.
    Install(InstallArgs),
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
    Remove { name: String },
}

#[derive(Subcommand)]
enum WalletCommand {
    /// Show wallet path, caller, permissions, and target status.
    Status(WalletStatusArgs),
    /// Point config at an existing plaintext wallet JSON file.
    Attach(WalletAttachArgs),
    /// Import a private key into a normalized wallet JSON file.
    Import(WalletImportArgs),
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
    /// Use discovered values and defaults without prompting.
    #[arg(long)]
    yes: bool,
}

#[derive(Args)]
struct InstallArgs {
    /// Print stable machine-readable install guidance.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct OpenArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// Print raw JSON instead of table or compact receipt output.
    #[arg(long)]
    json: bool,
    /// Write exact read JSON-RPC request/response envelopes to a JSONL file.
    #[arg(long = "trace-rpc-json", value_name = "FILE")]
    trace_rpc_json: Option<PathBuf>,
    /// Trace detail: full, summary, request_only, or response_meta.
    #[arg(
        long = "trace-rpc-json-mode",
        value_enum,
        default_value_t = TraceRpcJsonMode::Full
    )]
    trace_rpc_json_mode: TraceRpcJsonMode,
    /// Execute SQL from a file. Use - to read stdin.
    #[arg(long = "sql-file", value_name = "FILE")]
    sql_file: Option<PathBuf>,
    /// Refuse to submit state-changing SQL.
    #[arg(long)]
    read_only: bool,
    /// SQL to run directly instead of opening the shell.
    sql: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum TraceRpcJsonMode {
    Full,
    Summary,
    #[value(name = "request_only")]
    RequestOnly,
    #[value(name = "response_meta")]
    ResponseMeta,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ReadModeArg {
    Sealed,
    Public,
}

impl From<ReadModeArg> for ReadMode {
    fn from(value: ReadModeArg) -> Self {
        match value {
            ReadModeArg::Sealed => ReadMode::Sealed,
            ReadModeArg::Public => ReadMode::Public,
        }
    }
}

impl From<TraceRpcJsonMode> for RpcTraceMode {
    fn from(value: TraceRpcJsonMode) -> Self {
        match value {
            TraceRpcJsonMode::Full => RpcTraceMode::Full,
            TraceRpcJsonMode::Summary => RpcTraceMode::Summary,
            TraceRpcJsonMode::RequestOnly => RpcTraceMode::RequestOnly,
            TraceRpcJsonMode::ResponseMeta => RpcTraceMode::ResponseMeta,
        }
    }
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
    name: Option<String>,
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
    /// Octra Circle read mode. Sealed requires signed reads; public allows unsigned reads.
    #[arg(long, value_enum, default_value = "sealed")]
    read_mode: ReadModeArg,
    /// Do not wait for Circle creation confirmation or initializer SQL receipts.
    #[arg(long)]
    no_wait: bool,
    /// Do not save a local database name.
    #[arg(long = "no-name")]
    no_name: bool,
    /// Make the new database the default database.
    #[arg(long)]
    default: bool,
    /// SQL to run after creating the database.
    #[arg(long)]
    sql: Option<String>,
    /// Schema SQL file to run after creating the database.
    #[arg(long = "schema", value_name = "FILE")]
    read: Option<PathBuf>,
    /// Write a database deployment manifest.
    #[arg(long, value_name = "FILE")]
    manifest: Option<PathBuf>,
    /// Print a stable JSON summary.
    #[arg(long)]
    json: bool,
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
    /// Patch bundled WASM for the current owner without reading auth_info.
    #[arg(long)]
    bootstrap_owner: bool,
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
    /// Exit nonzero unless live database readiness checks pass.
    #[arg(long)]
    ready: bool,
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
struct WalletStatusArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// Print a stable JSON summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct WalletAttachArgs {
    /// Existing plaintext wallet JSON path.
    #[arg(value_name = "PATH")]
    path: PathBuf,
    /// Print a stable JSON summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct WalletImportArgs {
    /// Plain wallet JSON to normalize, or omit with --stdin to read a private key.
    #[arg(value_name = "PATH")]
    source: Option<PathBuf>,
    /// Read a private key from stdin.
    #[arg(long)]
    stdin: bool,
    /// Destination wallet JSON path. Defaults to the configured wallet path or ~/.octra/wallet.json.
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
    /// Do not make the imported wallet active in config.
    #[arg(long)]
    no_use: bool,
    /// Overwrite the destination wallet file if it exists.
    #[arg(long)]
    force: bool,
    /// Print a stable JSON summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct RestoreArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// SQL dump/script to restore. Use - or omit to read stdin.
    #[arg(long = "file", value_name = "FILE")]
    file: Option<PathBuf>,
    /// Print the full stable JSON restore envelope.
    #[arg(long)]
    json: bool,
    /// Print compact stable JSON with totals and transaction hash summary.
    #[arg(long)]
    json_summary: bool,
    /// Submit only the first restore batch with saved owner bootstrap metadata.
    #[arg(long)]
    bootstrap_owner: bool,
    /// Include full SQL text in restore batch errors. Off by default.
    #[arg(long)]
    verbose_sql: bool,
}

#[derive(Args)]
struct CheckArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// SQL to check.
    #[arg(long)]
    sql: Option<String>,
    /// SQL file to check. Use - to read stdin.
    #[arg(long = "sql-file", value_name = "FILE")]
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

#[derive(Args)]
struct CommandsArgs {
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
    let code = run_with_exit_code()?;
    if code == 0 {
        Ok(())
    } else {
        bail!("command exited with status {code}")
    }
}

pub fn run_with_exit_code() -> Result<i32> {
    let args = normalize_args(env::args().collect());
    let cli = Cli::parse_from(args);
    match cli.command {
        Commands::Setup(args) => cmd_setup(args).map(|_| 0),
        Commands::New(args) => cmd_new(args).map(|_| 0),
        Commands::Database { command } => cmd_database(command).map(|_| 0),
        Commands::Open(args) => cmd_open(args).map(|_| 0),
        Commands::Restore(args) => cmd_restore(args).map(|_| 0),
        Commands::Check(args) => cmd_check(args).map(|_| 0),
        Commands::Limits(args) => cmd_limits(args).map(|_| 0),
        Commands::CommandList(args) => cmd_commands(args).map(|_| 0),
        Commands::Verify(args) => {
            let session = build_session(&args.target)?;
            verify(
                &session,
                args.expected_hash.as_deref(),
                args.write_smoke,
                args.integrity,
                args.json,
            )
            .map(|_| 0)
        }
        Commands::Status(args) => cmd_status(args, "status"),
        Commands::Config(args) => cmd_config(args).map(|_| 0),
        Commands::Wallet { command } => cmd_wallet(command).map(|_| 0),
        Commands::Deploy(args) => cmd_deploy(args).map(|_| 0),
        Commands::Install(args) => cmd_install(args).map(|_| 0),
    }
}

fn cmd_install(args: InstallArgs) -> Result<()> {
    let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let local_install = "cargo install --path . --locked";
    let pinned_install = format!("cargo install --git {REPO_URL} --tag {tag} --locked");
    let setup = "octra-sqlite setup";
    let query = "octra-sqlite DATABASE \"select * from sqlite_schema;\"";
    let ready = "octra-sqlite status DATABASE --ready";
    if args.json {
        return print_json(&json!({
            "ok": true,
            "type": "install",
            "schema": "octra-sqlite.cli.v1",
            "rust": {
                "minimum": MIN_RUST_VERSION,
                "recommended": "rustup stable",
            },
            "commands": {
                "local": local_install,
                "pinned": pinned_install,
                "setup": setup,
                "create": CREATE_DATABASE_COMMAND,
                "query": query,
                "ready": ready,
            },
            "discovery": {
                "commands": "octra-sqlite commands --json",
                "limits": "octra-sqlite limits art --json",
            }
        }));
    }
    print_field(
        "rust",
        format!("{MIN_RUST_VERSION}+; rustup stable recommended"),
    );
    print_command("local", local_install);
    print_command("pinned", pinned_install);
    print_command("setup", setup);
    print_command("create", CREATE_DATABASE_COMMAND);
    print_command("query", query);
    print_command("ready", ready);
    Ok(())
}

fn normalize_args(mut args: Vec<String>) -> Vec<String> {
    const KNOWN: &[&str] = &[
        "setup",
        "new",
        "database",
        "open",
        "restore",
        "check",
        "limits",
        "commands",
        "verify",
        "status",
        "config",
        "wallet",
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

fn cmd_setup(args: SetupArgs) -> Result<()> {
    let mut config = load_config().unwrap_or_default();
    let interactive = !args.yes && io::stdin().is_terminal();
    if !interactive && !args.yes {
        bail!("setup is interactive; run it in a terminal or pass --yes with flags");
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
    reject_encrypted_oct_wallet(&wallet_path)?;
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

    config.wallet = Some(wallet_path.to_string_lossy().to_string());
    config.network = Some(network.clone());
    config.apply_active_network_profile();
    config.rpc = Some(rpc.clone());
    write_config(&config)?;
    print_field("wrote", config_path()?.display().to_string());
    print_field("wallet", wallet_path.display().to_string());
    print_field("network", &network);
    print_field("rpc", &rpc);
    if let Some(explorer) = config.explorer_for_network(&network) {
        print_field("explorer", explorer);
    }
    print_command("create", CREATE_DATABASE_COMMAND);
    if !wallet_path.is_file() {
        print_command(
            "wallet attach",
            format!(
                "octra-sqlite wallet attach {}",
                shell_quote_path(&wallet_path)
            ),
        );
        print_command(
            "wallet import",
            format!(
                "octra-sqlite wallet import --stdin --output {}",
                shell_quote_path(&wallet_path)
            ),
        );
    }
    Ok(())
}

fn cmd_new(args: NewArgs) -> Result<()> {
    let args = resolve_new_args(args)?;
    let name = args
        .name
        .as_deref()
        .ok_or_else(|| anyhow!("database name is required"))?;

    let mut config = load_config().unwrap_or_default();
    ensure_new_database_name_available(&args, &config, name)?;
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
        print_field("funding", funding_detail);
    }
    let read_mode = ReadMode::from(args.read_mode);
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
    print_field("open", format!("octra-sqlite open {followup_target}"));
    Ok(())
}

fn resolve_new_args(mut args: NewArgs) -> Result<NewArgs> {
    if args.name.is_some() {
        return Ok(args);
    }
    if args.json {
        bail!("database name is required with --json");
    }
    if !io::stdin().is_terminal() {
        bail!("database name is required; pass DATABASE or run octra-sqlite new in a terminal");
    }

    let config = load_config().unwrap_or_default();
    println!("{}", strong("Create an Octra SQLite database"));
    let name = prompt_required("Database name")?;
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
    if args.wallet.is_none() {
        args.wallet = config
            .wallet
            .as_ref()
            .map(PathBuf::from)
            .or_else(discover_wallet_path);
    }
    if !args.no_name && !args.default {
        args.default = true;
    }
    if args.manifest.is_none() {
        args.manifest = Some(default_new_manifest_path(&name));
    }
    if !prompt_yes_no("Create database?", true)? {
        bail!("cancelled");
    }
    Ok(args)
}

fn default_new_manifest_path(name: &str) -> PathBuf {
    PathBuf::from(format!("{name}.octra-sqlite.json"))
}

fn database_read_uri(target_uri: &str, read_mode: ReadMode) -> String {
    match read_mode {
        ReadMode::Public => format!("{target_uri}?read_mode=public"),
        ReadMode::Auto | ReadMode::Sealed => target_uri.to_string(),
    }
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

fn run_initializer_sql(
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

fn save_new_database_name(
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

fn ensure_new_database_name_available(args: &NewArgs, config: &Config, name: &str) -> Result<()> {
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
    bail!("database name '{name}' already exists for database URI {existing_uri}");
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

struct NewManifestInput<'a> {
    args: &'a NewArgs,
    name: &'a str,
    target_uri: &'a str,
    network: &'a str,
    created: &'a CreatedCircle,
    owner: &'a str,
    rpc: &'a str,
    init_sql: &'a [String],
    initializer_results: &'a [SqlScriptExecution],
    readiness: Value,
}

fn new_manifest_json(input: NewManifestInput<'_>) -> Value {
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

fn write_new_manifest(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, format_json(value)? + "\n")
        .with_context(|| format!("writing {}", path.display()))
}

fn new_readiness_skipped_json() -> Value {
    json!({
        "checked": false,
        "reason": "no_wait",
    })
}

fn new_readiness_json(session: &Session) -> Value {
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

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.display().to_string())
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

fn print_command(label: &str, command: impl AsRef<str>) {
    println!("{} {}", dim(format!("{label}:")), command.as_ref());
}

fn cmd_status(args: StatusArgs, label: &str) -> Result<i32> {
    let mut report = StatusReport::new(label, args.json);
    report.init_database_readiness();
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
    report.finish_with_ready(label, args.ready)
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

fn cmd_wallet(command: WalletCommand) -> Result<()> {
    match command {
        WalletCommand::Status(args) => cmd_wallet_status(args),
        WalletCommand::Attach(args) => cmd_wallet_attach(args),
        WalletCommand::Import(args) => cmd_wallet_import(args),
    }
}

fn cmd_wallet_attach(args: WalletAttachArgs) -> Result<()> {
    reject_encrypted_oct_wallet(&args.path)?;
    let path = canonical_existing_wallet_path(&args.path)?;
    let material = wallet_file_material(&path)?;
    let mut config = load_config().unwrap_or_default();
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

fn cmd_wallet_import(args: WalletImportArgs) -> Result<()> {
    let mut config = load_config().unwrap_or_default();
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
        let mut private_key =
            read_stdin_secret("wallet import --stdin requires a private key on stdin")?;
        let material = wallet_material_from_private_key(&private_key, None)?;
        private_key.zeroize();
        material
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
    print_field("next", "octra-sqlite wallet status");
    Ok(())
}

fn cmd_wallet_status(args: WalletStatusArgs) -> Result<()> {
    let mut report = StatusReport::new("wallet_status", args.json);
    let config = load_config().unwrap_or_default();
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

fn reject_encrypted_oct_wallet(path: &Path) -> Result<()> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("oct"))
    {
        bail!(
            "webcli .oct wallets are encrypted and need PIN-based decryption; export/import the private key with `octra-sqlite wallet import --stdin` or attach a plaintext wallet JSON"
        );
    }
    Ok(())
}

fn canonical_existing_wallet_path(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).with_context(|| format!("wallet not found at {}", path.display()))
}

fn default_wallet_output_path() -> Result<PathBuf> {
    Ok(config_path()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wallet.json"))
}

fn absolute_wallet_output_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(env::current_dir()?.join(path))
}

fn read_stdin_secret(error: &str) -> Result<String> {
    if io::stdin().is_terminal() {
        bail!("{error}");
    }
    let mut secret = String::new();
    io::stdin().read_to_string(&mut secret)?;
    Ok(secret)
}

fn write_wallet_json(path: &Path, material: &WalletMaterial, force: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = json!({
        "address": &material.address,
        "private_key_b64": &material.private_key_b64,
        "public_key_b64": &material.public_key_b64,
    });
    let mut text = serde_json::to_string_pretty(&payload)? + "\n";
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("writing wallet {}", path.display()))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    text.zeroize();
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn report_wallet_permissions(report: &mut StatusReport, path: &Path) {
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

struct StatusReport {
    label: String,
    json: bool,
    failures: usize,
    warnings: usize,
    items: Vec<Value>,
    readiness: Map<String, Value>,
}

impl StatusReport {
    fn new(label: &str, json: bool) -> Self {
        Self {
            label: label.to_string(),
            json,
            failures: 0,
            warnings: 0,
            items: Vec::new(),
            readiness: Map::new(),
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

    fn ready(&mut self, key: &str, ready: bool) {
        self.readiness.insert(key.to_string(), Value::Bool(ready));
    }

    fn init_database_readiness(&mut self) {
        for key in DATABASE_READINESS_KEYS {
            self.readiness.insert(key.to_string(), Value::Null);
        }
    }

    fn finish(self, label: &str) -> Result<()> {
        self.finish_with_ready(label, false).map(|_| ())
    }

    fn finish_with_ready(self, label: &str, require_ready: bool) -> Result<i32> {
        let database_ready = self.database_ready();
        let ok = self.failures == 0 && (!require_ready || database_ready);
        if self.json {
            return print_json(&json!({
                "ok": ok,
                "type": self.label,
                "schema": "octra-sqlite.cli.v1",
                "ready": database_ready,
                "failures": self.failures,
                "warnings": self.warnings,
                "readiness": self.readiness,
                "items": self.items,
            }))
            .map(|_| if ok { 0 } else { 1 });
        }
        if self.failures != 0 {
            bail!("{label} found {} issue(s)", self.failures)
        } else if require_ready && !database_ready {
            println!("{}", format_status_line("fail", "ready", "false"));
            Ok(1)
        } else {
            println!("{} ready", dim(format!("{label}:")));
            Ok(0)
        }
    }

    fn database_ready(&self) -> bool {
        DATABASE_READINESS_KEYS
            .iter()
            .all(|key| self.readiness.get(*key).and_then(Value::as_bool) == Some(true))
    }
}

const DATABASE_READINESS_KEYS: &[&str] = &[
    "circle_reachable",
    "auth_readable",
    "owner_write_valid",
    "storage_initialized",
    "sqlite_ready",
    "query_ready",
];

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
        Err(error) => {
            if session.target().read_mode.allows_unsigned_read() {
                match circle_info(session) {
                    Ok(info) => {
                        report.ready("circle_reachable", true);
                        report.warn(
                            "program info",
                            format!("signed program info unavailable: {error}"),
                        );
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
        Ok(result) => {
            report.ready("sqlite_ready", true);
            report.ready("query_ready", true);
            report.ok(
                "sqlite version",
                first_result_cell(&result).unwrap_or_else(|| value_to_string(&result)),
            );
        }
        Err(error) => {
            report.ready("sqlite_ready", false);
            report.ready("query_ready", false);
            report.fail("sqlite version", error.to_string());
        }
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
    owner: String,
    code_hash: String,
    code_bytes: usize,
    auth_patch: AuthPatch,
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

fn create_circle(
    session: &Session,
    args: &NewArgs,
    network: &str,
    read_mode: ReadMode,
) -> Result<CreatedCircle> {
    let wasm_path = resolve_wasm_for_new(args)?;
    let mut wasm =
        fs::read(&wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;
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

fn resolve_bundled_wasm_path() -> Result<PathBuf> {
    find_project_file(DEFAULT_WASM_REL).ok_or_else(|| {
        anyhow!("could not find bundled {DEFAULT_WASM_REL}; restore the repo artifact")
    })
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

fn circle_deploy_payload_json(code_b64: Option<&str>, read_mode: ReadMode) -> Result<String> {
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

fn deploy_tuple(read_mode: ReadMode) -> (&'static str, &'static str, &'static str) {
    match read_mode {
        ReadMode::Public => ("public", "gateway_allowed", "public_resources"),
        ReadMode::Auto | ReadMode::Sealed => ("sealed", "native_sealed", "sealed_read"),
    }
}

fn circle_id_of_deploy(deployer: &str, nonce: u64, payload_json: &str) -> String {
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

fn print_database_list(config: &Config, json_mode: bool) -> Result<()> {
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

fn print_database_info(config: &Config, database: Option<&str>, json_mode: bool) -> Result<()> {
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

fn cmd_open(args: OpenArgs) -> Result<()> {
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
    if let Some(path) = &args.sql_file {
        let sql = read_sql_file_arg(path)?;
        return run_sql_input(&session, &sql, mode, true, args.read_only, trace_rpc_json);
    }
    if args.sql.is_empty() {
        if let Some(sql) = read_stdin_sql()? {
            return run_sql_input(&session, &sql, mode, true, args.read_only, trace_rpc_json);
        }
        if args.trace_rpc_json.is_some() {
            bail!("--trace-rpc-json requires one SQL statement; interactive shell tracing is not supported");
        }
        run_shell(session, mode)
    } else {
        let sql = args.sql.join(" ");
        run_sql_input(&session, &sql, mode, true, args.read_only, trace_rpc_json)
    }
}

fn cmd_restore(args: RestoreArgs) -> Result<()> {
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

struct BootstrapPostAuthReport<'a> {
    session: &'a Session,
    plan: &'a SqlScriptPlan,
    execution: &'a SqlScriptExecution,
    progress: &'a [SqlBatchProgress],
    mode: Option<&'a BootstrapOwnerMode>,
    json_summary: bool,
    json_full: bool,
    post_auth_error: &'a str,
}

fn report_bootstrap_post_auth_failure(report: BootstrapPostAuthReport<'_>) -> Result<()> {
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
    bail!(
        "bootstrap first write was submitted but post-write auth_info still failed; first_write={}; post_auth_info_error={post_auth_error}",
        serde_json::to_string(&first_write)?,
        post_auth_error = report.post_auth_error
    )
}

#[derive(Clone, Debug)]
enum BootstrapOwnerMode {
    FirstWrite(BootstrapOwnerMetadata),
    AlreadyBootstrapped,
}

#[derive(Clone, Debug)]
struct BootstrapOwnerMetadata {
    uri: String,
    owner: String,
    owner_pubkey: String,
    db_id: String,
    code_hash: String,
}

fn resolve_bootstrap_owner_mode(
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

fn find_bootstrap_owner_metadata(session: &Session) -> Result<BootstrapOwnerMetadata> {
    let config = load_config().unwrap_or_default();
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

fn bootstrap_owner_personalized_hash(metadata: &BootstrapOwnerMetadata) -> Result<String> {
    let wasm_path = resolve_bundled_wasm_path()?;
    let mut wasm =
        fs::read(&wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;
    let owner_pubkey = hex_to_32("owner_pubkey", &metadata.owner_pubkey)?;
    let db_id = hex_to_32("db_id", &metadata.db_id)?;
    patch_wasm_auth_bytes(&mut wasm, &owner_pubkey, &db_id)?;
    Ok(sha256_hex(&wasm))
}

fn is_empty_storage_cache_error(text: &str) -> bool {
    const ZERO_ROOT: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    text.contains("missing storage cache") && text.contains(ZERO_ROOT)
}

fn add_bootstrap_owner_json(mut value: Value, mode: Option<&BootstrapOwnerMode>) -> Value {
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
    print_field("max result rows", MAX_RESULT_ROWS.to_string());
    print_field("transactions", "one accepted exec is atomic");
    print_field("user BEGIN/COMMIT", "unsupported across Octra writes");
    print_field(
        "restore",
        "chunked; multi-batch restore can partially apply",
    );
    print_field("reads", "signed Octra view auth");
    print_field("writes", "OSW1 owner write intent");
    print_field("read-only", "client guard via --read-only");
    print_field("trace modes", "full, summary, request_only, response_meta");
    if let Some(target) = target {
        print_field("database", target["uri"].as_str().unwrap_or(""));
        print_field("network", target["network"].as_str().unwrap_or(""));
        print_field("circle", target["circle"].as_str().unwrap_or(""));
    }
    Ok(())
}

fn cmd_commands(args: CommandsArgs) -> Result<()> {
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
            bail!("read_only: dot command may write to the database");
        }
        run_dot_command(session.clone(), mode, headers, output, trimmed)?;
        return Ok(());
    }
    if looks_like_sql_script(sql) {
        if trace_rpc_json.is_some() {
            bail!("--trace-rpc-json supports one read-only SQL statement, not SQL scripts");
        }
        if read_only {
            bail!("read_only: multi-statement SQL scripts are not submitted in read-only mode");
        }
        return run_exec_script_to(session, sql, mode, output);
    }
    ensure_sql_text_fits(sql)?;
    let query_result = match trace_rpc_json {
        Some((path, mode)) => query_typed_traced(session, sql, path, mode),
        None => query_typed(session, sql),
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

fn restore_summary_envelope(
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
        "read_mode": target.read_mode.as_str(),
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
            "read_model": "sealed uses signed Octra view auth; public uses unsigned Octra circle view",
            "read_modes": ["sealed", "public"],
            "sealed_reads": "octra_circleViewAuth",
            "public_reads": "octra_circleView",
            "write_model": "OSW1 owner write intent",
            "read_only_guard": "client-side --read-only",
            "native_roles": false,
        },
        "trace": {
            "default": "off",
            "option": "--trace-rpc-json FILE",
            "modes": ["full", "summary", "request_only", "response_meta"],
            "mode_option": "--trace-rpc-json-mode MODE",
        }
    })
}

fn commands_json() -> Value {
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
                "purpose": "interactive wallet and network setup",
                "writes": false,
                "json": false,
            },
            {
                "command": "octra-sqlite new [DATABASE] [SQL]",
                "purpose": "create a Circle-backed SQLite database; prompts when DATABASE is omitted in a terminal",
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
                "purpose": "run one SQL statement or script against a database",
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
                "purpose": "open the interactive sqlite> shell",
                "writes": "depends_on_sql",
                "json": false,
            },
            {
                "command": "octra-sqlite restore DATABASE --file dump.sql",
                "purpose": "restore large SQL text with chunked execution",
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
                "purpose": "exit nonzero unless live database readiness checks pass",
                "writes": false,
                "json": true,
                "envelope": "status",
            },
            {
                "command": "octra-sqlite verify [DATABASE]",
                "purpose": "verify live Circle SQLite status and optional integrity/write checks",
                "writes": "optional",
                "json": true,
                "envelope": "verify",
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
                "command": "octra-sqlite database use NAME",
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
                "purpose": "normalize a plaintext wallet or stdin private key into a local wallet JSON",
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
            {
                "command": "octra-sqlite install",
                "purpose": "print installation instructions for the Rust CLI",
                "writes": false,
                "json": true,
                "envelope": "install",
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
            "install",
            "verify",
            "database_list",
            "database_info",
            "error"
        ],
        "discovery": {
            "install": "octra-sqlite install --json",
            "limits": "octra-sqlite limits DATABASE --json",
            "status": "octra-sqlite status DATABASE --json",
            "wallet": "octra-sqlite wallet status DATABASE --json",
            "json_docs": "docs/json-output.md",
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
        "artists" => Ok(include_str!("../../examples/artists.sql").to_string()),
        "remilia" => Ok(include_str!("../../examples/remilia-collections.sql").to_string()),
        _ => bail!("unknown sample {name}; available samples: artists, remilia"),
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

fn prompt_required(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{label} is required");
    }
    Ok(trimmed.to_string())
}

fn prompt_path(label: &str, default: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(prompt_default(
        label,
        &default.to_string_lossy(),
    )?))
}

fn prompt_read_mode(default: ReadModeArg) -> Result<ReadModeArg> {
    let default_text = match default {
        ReadModeArg::Sealed => "sealed",
        ReadModeArg::Public => "public",
    };
    let value = prompt_default("Read mode (sealed/public)", default_text)?;
    match value.trim().to_ascii_lowercase().as_str() {
        "sealed" => Ok(ReadModeArg::Sealed),
        "public" => Ok(ReadModeArg::Public),
        _ => bail!("read mode must be sealed or public"),
    }
}

fn prompt_network(default: &str) -> Result<String> {
    let value = prompt_default("Network (devnet/mainnet)", default)?;
    match value.trim().to_ascii_lowercase().as_str() {
        "devnet" => Ok("devnet".to_string()),
        "mainnet" => Ok("mainnet".to_string()),
        _ => bail!("network must be devnet or mainnet"),
    }
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
        let mut target = resolve_target(database, config)?;
        apply_target_metadata(value, config, &mut target);
        return Ok(target);
    }
    parse_target_uri(value, config)
}

fn parse_target_uri(value: &str, config: &Config) -> Result<Target> {
    let mut target = parse_database_target(value, config.network.as_deref(), None)?;
    if target.rpc.is_empty() {
        target.rpc = config.rpc_for_network(&target.network).unwrap_or_default();
    }
    apply_target_metadata(value, config, &mut target);
    Ok(target)
}

fn apply_target_metadata(requested: &str, config: &Config, target: &mut Target) {
    if let Some(metadata) = config.metadata_for_target(requested, target) {
        target.read_mode = metadata.read_mode;
    }
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
    let wasm_path = if args.bootstrap_owner {
        resolve_bundled_wasm_path()?
    } else {
        resolve_wasm_path(args.build, args.wasm.as_deref())?
    };
    let mut wasm =
        fs::read(&wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;
    if args.bootstrap_owner && args.allow_unconfigured {
        bail!("--bootstrap-owner and --allow-unconfigured are mutually exclusive");
    }
    let auth_patch = if args.bootstrap_owner {
        let info = program_info(&session)
            .context("reading Circle program info before owner bootstrap deploy")?;
        match program_owner(&info) {
            Some(owner) if owner == session.caller() => {}
            Some(owner) => bail!(
                "Circle owner is {owner}; current wallet {} cannot bootstrap owner-personalized WASM",
                session.caller()
            ),
            None => bail!("Circle program info did not expose an owner; refusing bootstrap deploy"),
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
    out.insert(
        "wasm".to_string(),
        Value::String(wasm_path.display().to_string()),
    );
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

fn save_bootstrap_owner_metadata(
    session: &Session,
    patch: &AuthPatch,
    code_hash: &str,
    code_bytes: usize,
    tx_hash: Option<String>,
) -> Result<Vec<String>> {
    let mut config = load_config().unwrap_or_default();
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
                owner_pubkey: patch.owner_pubkey_hex.clone(),
                db_id: patch.db_id_hex.clone(),
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
    use std::collections::BTreeSet;

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

        let commands = Cli::try_parse_from(["octra-sqlite", "commands", "--json"]).unwrap();
        match commands.command {
            Commands::CommandList(args) => assert!(args.json),
            _ => panic!("expected commands command"),
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
            bootstrap_owner: false,
            wallet: None,
            caller: None,
            private_key_b64: None,
            public_key_b64: None,
        };
        let error = cmd_deploy(args).unwrap_err().to_string();
        assert!(error.contains("requires --circle"));
    }

    #[test]
    fn deploy_accepts_owner_bootstrap_for_explicit_circle() {
        let cli = Cli::try_parse_from([
            "octra-sqlite",
            "deploy",
            "--circle",
            "oct://mainnet/octABC",
            "--bootstrap-owner",
        ])
        .unwrap();
        match cli.command {
            Commands::Deploy(args) => {
                assert_eq!(args.circle.as_deref(), Some("oct://mainnet/octABC"));
                assert!(args.bootstrap_owner);
                assert!(!args.allow_unconfigured);
            }
            _ => panic!("expected deploy command"),
        }
    }

    #[test]
    fn restore_accepts_owner_bootstrap_for_explicit_uri() {
        let cli = Cli::try_parse_from([
            "octra-sqlite",
            "restore",
            "oct://mainnet/octABC",
            "--file",
            "schema.sql",
            "--bootstrap-owner",
            "--verbose-sql",
            "--json-summary",
        ])
        .unwrap();
        match cli.command {
            Commands::Restore(args) => {
                assert_eq!(args.target.target.as_deref(), Some("oct://mainnet/octABC"));
                assert_eq!(args.file, Some(PathBuf::from("schema.sql")));
                assert!(args.bootstrap_owner);
                assert!(args.verbose_sql);
                assert!(args.json_summary);
            }
            _ => panic!("expected restore command"),
        }
    }

    #[test]
    fn bootstrap_owner_only_accepts_empty_storage_cache_errors() {
        let zero_root = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(is_empty_storage_cache_error(&format!(
            "octra_circleViewAuth failed: missing storage cache: octABC:{zero_root}"
        )));
        assert!(!is_empty_storage_cache_error(
            "octra_circleViewAuth failed: missing storage cache: octABC:1111111111111111111111111111111111111111111111111111111111111111"
        ));
        assert!(!is_empty_storage_cache_error(
            "octra_circleViewAuth failed: wasm export returned 1"
        ));
    }

    #[test]
    fn bootstrap_owner_json_marks_first_write_recovery() {
        let metadata = BootstrapOwnerMetadata {
            uri: "oct://mainnet/octABC".to_string(),
            owner: "octOwner".to_string(),
            owner_pubkey: "aa".repeat(32),
            db_id: "bb".repeat(32),
            code_hash: "cc".repeat(32),
        };
        let mode = BootstrapOwnerMode::FirstWrite(metadata);
        let value = add_bootstrap_owner_json(json!({"ok": true}), Some(&mode));
        assert_eq!(value["bootstrap_owner"], true);
        assert_eq!(value["bootstrap"]["mode"], "owner_first_write");
        assert_eq!(value["bootstrap"]["reason"], "empty_storage_cache");
        assert_eq!(value["bootstrap"]["uri"], "oct://mainnet/octABC");
    }

    #[test]
    fn bootstrap_owner_json_marks_already_bootstrapped_restore() {
        let mode = BootstrapOwnerMode::AlreadyBootstrapped;
        let value = add_bootstrap_owner_json(json!({"ok": true}), Some(&mode));
        assert_eq!(value["bootstrap_owner"], true);
        assert_eq!(value["bootstrap"]["mode"], "normal_restore");
        assert_eq!(value["bootstrap"]["reason"], "already_bootstrapped");
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
    fn install_accepts_json() {
        let cli = Cli::try_parse_from(["octra-sqlite", "install", "--json"]).unwrap();
        match cli.command {
            Commands::Install(args) => assert!(args.json),
            _ => panic!("expected install command"),
        }
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
                assert_eq!(args.name.as_deref(), Some("my-db"));
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
    fn open_accepts_read_rpc_trace_path() {
        let cli = Cli::try_parse_from([
            "octra-sqlite",
            "open",
            "art",
            "--trace-rpc-json",
            "trace.jsonl",
            "--trace-rpc-json-mode",
            "summary",
            "select * from artist;",
        ])
        .unwrap();
        match cli.command {
            Commands::Open(args) => {
                assert_eq!(args.target.target.as_deref(), Some("art"));
                assert_eq!(args.trace_rpc_json, Some(PathBuf::from("trace.jsonl")));
                assert_eq!(args.trace_rpc_json_mode, TraceRpcJsonMode::Summary);
                assert_eq!(args.sql, vec!["select * from artist;"]);
            }
            _ => panic!("expected open command"),
        }
    }

    #[test]
    fn trace_mode_requires_trace_path() {
        let args = OpenArgs {
            target: TargetArgs {
                target: Some("oct://devnet/octABC".to_string()),
                wallet: None,
                rpc: Some("mock://rpc".to_string()),
                caller: Some("octCaller".to_string()),
                private_key_b64: Some(
                    "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
                ),
                public_key_b64: None,
            },
            json: false,
            trace_rpc_json: None,
            trace_rpc_json_mode: TraceRpcJsonMode::Summary,
            sql_file: None,
            read_only: false,
            sql: vec!["select 1;".to_string()],
        };
        let error = cmd_open(args).unwrap_err().to_string();
        assert!(error.contains("--trace-rpc-json-mode requires --trace-rpc-json"));
    }

    #[test]
    fn restore_summary_envelope_omits_per_batch_receipts() {
        let session = build_session(&TargetArgs {
            target: Some("oct://devnet/octABC".to_string()),
            wallet: None,
            rpc: Some("mock://rpc".to_string()),
            caller: Some("octCaller".to_string()),
            private_key_b64: Some(
                "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
            ),
            public_key_b64: None,
        })
        .unwrap();
        let plan = SqlScriptPlan {
            source_bytes: 42,
            total_statements: 2,
            executable_statements: 2,
            skipped_statements: 0,
            batches: 2,
            max_statement_bytes: 21,
            max_payload_bytes: 21,
        };
        let execution = SqlScriptExecution {
            statements: 2,
            batches: 2,
            results: vec![
                json!({"tx_hash": "tx1", "tx_url": "https://example/tx1", "receipt": {"success": true}}),
                json!({"tx_hash": "tx2", "tx_url": "https://example/tx2", "receipt": {"success": true}}),
            ],
        };
        let envelope = restore_summary_envelope(&session, &plan, &execution);
        assert_eq!(envelope["type"], "restore");
        assert_eq!(envelope["summary"], true);
        assert_eq!(envelope["writes"]["total"], 2);
        assert_eq!(envelope["writes"]["confirmed"], 2);
        assert_eq!(envelope["writes"]["first_tx_hash"], "tx1");
        assert_eq!(envelope["writes"]["last_tx_hash"], "tx2");
        assert!(envelope.get("progress").is_none());
    }

    #[test]
    fn limits_json_exposes_automation_contract_facts() {
        let limits = limits_json(None);
        assert_eq!(limits["ok"], true);
        assert_eq!(limits["type"], "limits");
        assert_eq!(limits["schema"], "octra-sqlite.cli.v1");
        assert_eq!(limits["versions"]["sqlite"], SQLITE_VERSION);
        assert_eq!(limits["sql"]["max_sql_bytes"], MAX_SQL_TEXT_BYTES);
        assert_eq!(limits["result"]["max_rows"], MAX_RESULT_ROWS);
        assert_eq!(limits["result"]["limit_error"], "result_limit_exceeded");
        assert_eq!(
            limits["auth"]["read_model"],
            "sealed uses signed Octra view auth; public uses unsigned Octra circle view"
        );
        assert_eq!(limits["auth"]["read_modes"], json!(["sealed", "public"]));
        assert_eq!(limits["auth"]["write_model"], "OSW1 owner write intent");
        assert!(limits["trace"]["modes"]
            .as_array()
            .unwrap()
            .contains(&json!("summary")));
    }

    #[test]
    fn commands_json_lists_public_cli_surface() {
        let commands = commands_json();
        assert_eq!(commands["ok"], true);
        assert_eq!(commands["type"], "commands");
        assert_eq!(commands["schema"], "octra-sqlite.cli.v1");
        assert!(commands["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| {
                command.get("command").and_then(Value::as_str)
                    == Some("octra-sqlite DATABASE \"SQL\"")
                    && command
                        .get("envelopes")
                        .and_then(Value::as_array)
                        .unwrap()
                        .contains(&json!("query"))
            }));
        assert!(commands["json_envelopes"]
            .as_array()
            .unwrap()
            .contains(&json!("new")));
        assert!(commands["json_envelopes"]
            .as_array()
            .unwrap()
            .contains(&json!("install")));
        assert_eq!(
            commands["discovery"]["install"],
            "octra-sqlite install --json"
        );
        assert_eq!(
            commands["discovery"]["limits"],
            "octra-sqlite limits DATABASE --json"
        );
    }

    #[test]
    fn commands_json_covers_every_public_top_level_command() {
        let commands = commands_json();
        let catalog = commands["commands"].as_array().unwrap();
        let catalog_names = catalog
            .iter()
            .filter_map(|command| command.get("command").and_then(Value::as_str))
            .filter_map(|command| command.split_whitespace().nth(1))
            .filter(|name| *name != "DATABASE")
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        let clap_names = <Cli as clap::CommandFactory>::command()
            .get_subcommands()
            .map(|command| command.get_name().to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            catalog_names, clap_names,
            "commands --json must cover every public top-level command"
        );
    }

    fn test_new_args(name: &str) -> NewArgs {
        NewArgs {
            name: Some(name.to_string()),
            build: false,
            wasm: None,
            create_ou: "200000".to_string(),
            rpc: None,
            network: Some("devnet".to_string()),
            read_mode: ReadModeArg::Sealed,
            no_wait: false,
            no_name: false,
            default: false,
            sql: None,
            read: None,
            manifest: None,
            json: true,
            sample: None,
            wallet: None,
            caller: None,
            private_key_b64: None,
            public_key_b64: None,
            sql_args: Vec::new(),
        }
    }

    #[test]
    fn new_manifest_uses_database_ontology() {
        let mut args = test_new_args("art");
        args.read = Some(PathBuf::from("schema.sql"));
        args.manifest = Some(PathBuf::from("art.json"));
        let created = CreatedCircle {
            circle: "octABC".to_string(),
            owner: "octOwner".to_string(),
            code_hash: "hash".to_string(),
            code_bytes: 123,
            auth_patch: AuthPatch {
                owner_pubkey_hex: "ownerpub".to_string(),
                db_id_hex: "dbid".to_string(),
                owner_pubkey_offset: 1,
                db_id_offset: 2,
            },
            tx_hash: Some("tx".to_string()),
            confirmation: None,
        };
        let init_sql = vec!["create table artist(id integer);".to_string()];
        let initializer_results = Vec::new();
        let manifest = new_manifest_json(NewManifestInput {
            args: &args,
            name: "art",
            target_uri: "oct://devnet/octABC",
            network: "devnet",
            created: &created,
            owner: "octOwner",
            rpc: "https://devnet.octrascan.io/rpc",
            init_sql: &init_sql,
            initializer_results: &initializer_results,
            readiness: json!({"checked": true, "ready": true}),
        });
        assert_eq!(manifest["manifest_version"], "octra-sqlite.database.v1");
        assert_eq!(manifest["database"]["name"], "art");
        assert_eq!(manifest["database"]["uri"], "oct://devnet/octABC");
        assert_eq!(manifest["database"]["read_uri"], "oct://devnet/octABC");
        assert_eq!(manifest["owner"]["write_auth"], "OSW1 owner write intent");
        assert_eq!(manifest["program"]["runtime"], "wasm_v1");
        assert_eq!(manifest["initializer"]["schema_file"], "schema.sql");
        assert!(manifest.get("app").is_none());
    }

    #[test]
    fn public_database_read_uri_is_shareable() {
        assert_eq!(
            database_read_uri("oct://devnet/octABC", ReadMode::Public),
            "oct://devnet/octABC?read_mode=public"
        );
        assert_eq!(
            database_read_uri("oct://devnet/octABC", ReadMode::Sealed),
            "oct://devnet/octABC"
        );
    }

    #[test]
    fn new_refuses_to_overwrite_existing_database_name() {
        let args = test_new_args("art");
        let mut config = Config::default();
        config
            .databases
            .insert("art".to_string(), "oct://devnet/octABC".to_string());
        let error = ensure_new_database_name_available(&args, &config, "art").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("already exists"));
        assert!(message.contains("oct://devnet/octABC"));
    }

    #[test]
    fn new_no_name_allows_existing_local_database_name() {
        let mut args = test_new_args("art");
        args.no_name = true;
        let mut config = Config::default();
        config
            .databases
            .insert("art".to_string(), "oct://devnet/octABC".to_string());
        ensure_new_database_name_available(&args, &config, "art").unwrap();
    }

    #[test]
    fn new_accepts_builtin_sample() {
        let cli =
            Cli::try_parse_from(["octra-sqlite", "new", "my-db", "--sample", "artists"]).unwrap();
        match cli.command {
            Commands::New(args) => {
                assert_eq!(args.name.as_deref(), Some("my-db"));
                assert_eq!(args.sample.as_deref(), Some("artists"));
            }
            _ => panic!("expected new command"),
        }
    }

    #[test]
    fn new_accepts_public_read_mode() {
        let cli =
            Cli::try_parse_from(["octra-sqlite", "new", "my-db", "--read-mode", "public"]).unwrap();
        match cli.command {
            Commands::New(args) => {
                assert_eq!(args.name.as_deref(), Some("my-db"));
                assert_eq!(ReadMode::from(args.read_mode), ReadMode::Public);
            }
            _ => panic!("expected new command"),
        }
    }

    #[test]
    fn new_accepts_wizard_mode_json_schema_and_manifest() {
        let cli = Cli::try_parse_from([
            "octra-sqlite",
            "new",
            "--schema",
            "schema.sql",
            "--manifest",
            "database.json",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Commands::New(args) => {
                assert!(args.name.is_none());
                assert_eq!(args.read.as_deref(), Some(Path::new("schema.sql")));
                assert_eq!(args.manifest.as_deref(), Some(Path::new("database.json")));
                assert!(args.json);
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
    fn setup_accepts_noninteractive_defaults() {
        let cli = Cli::try_parse_from(["octra-sqlite", "setup", "--yes"]).unwrap();
        match cli.command {
            Commands::Setup(args) => assert!(args.yes),
            _ => panic!("expected setup command"),
        }
    }

    #[test]
    fn setup_rejects_encrypted_oct_wallet_path() {
        let error = reject_encrypted_oct_wallet(Path::new("wallet.oct")).unwrap_err();
        assert!(error
            .to_string()
            .contains("webcli .oct wallets are encrypted"));
        let error = reject_encrypted_oct_wallet(Path::new("wallet.OCT")).unwrap_err();
        assert!(error
            .to_string()
            .contains("webcli .oct wallets are encrypted"));
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
    fn status_accepts_readiness_gate() {
        let cli =
            Cli::try_parse_from(["octra-sqlite", "status", "art", "--ready", "--json"]).unwrap();
        match cli.command {
            Commands::Status(args) => {
                assert_eq!(args.target.target.as_deref(), Some("art"));
                assert!(args.ready);
                assert!(args.json);
            }
            _ => panic!("expected status command"),
        }
    }

    #[test]
    fn status_readiness_requires_all_database_items() {
        let mut report = StatusReport::new("status", true);
        report.init_database_readiness();
        assert!(!report.database_ready());
        for key in DATABASE_READINESS_KEYS {
            report.ready(key, true);
        }
        assert!(report.database_ready());
        report.ready("sqlite_ready", false);
        assert!(!report.database_ready());
    }

    #[test]
    fn wallet_status_accepts_target_and_json() {
        let cli =
            Cli::try_parse_from(["octra-sqlite", "wallet", "status", "art", "--json"]).unwrap();
        match cli.command {
            Commands::Wallet {
                command: WalletCommand::Status(args),
            } => {
                assert_eq!(args.target.target.as_deref(), Some("art"));
                assert!(args.json);
            }
            _ => panic!("expected wallet status command"),
        }
    }

    #[test]
    fn wallet_attach_accepts_path_and_json() {
        let cli = Cli::try_parse_from([
            "octra-sqlite",
            "wallet",
            "attach",
            "./wallet.json",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Commands::Wallet {
                command: WalletCommand::Attach(args),
            } => {
                assert_eq!(args.path, PathBuf::from("./wallet.json"));
                assert!(args.json);
            }
            _ => panic!("expected wallet attach command"),
        }
    }

    #[test]
    fn wallet_import_accepts_stdin_output_and_json() {
        let cli = Cli::try_parse_from([
            "octra-sqlite",
            "wallet",
            "import",
            "--stdin",
            "--output",
            "./wallet.json",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Commands::Wallet {
                command: WalletCommand::Import(args),
            } => {
                assert!(args.stdin);
                assert_eq!(args.output.as_deref(), Some(Path::new("./wallet.json")));
                assert!(args.json);
            }
            _ => panic!("expected wallet import command"),
        }
    }

    #[test]
    fn wallet_is_not_treated_as_database_shorthand() {
        let args = normalize_args(vec![
            "octra-sqlite".to_string(),
            "wallet".to_string(),
            "status".to_string(),
        ]);
        assert_eq!(args, vec!["octra-sqlite", "wallet", "status"]);
    }

    #[test]
    fn remilia_sample_creates_expected_table() {
        let sql = sample_sql("artists").unwrap();
        assert!(sql.contains("create table artist"));
        assert!(sql.contains("Basquiat"));

        let sql = sample_sql("remilia").unwrap();
        assert!(sql.contains("create table collection"));
        assert!(sql.contains("Milady Maker"));
        assert!(!sql.contains("source_url"));
        assert!(!sql.contains("notes"));
        assert!(sample_sql("unknown").is_err());
    }

    #[test]
    fn deploy_payload_json_matches_wasm_v1_circle_shape() {
        let payload = circle_deploy_payload_json(None, ReadMode::Sealed).unwrap();
        assert_eq!(
            payload,
            "{\"runtime\":\"wasm_v1\",\"privacy_class\":\"sealed\",\"browser_mode\":\"native_sealed\",\"resource_mode\":\"sealed_read\",\"code_b64\":null,\"policy_hash\":null,\"members_root\":null,\"export_policy\":null,\"limits\":{\"max_stable_bytes\":\"33554432\",\"max_assets_bytes\":\"33554432\",\"max_inline_value\":\"65536\",\"max_wasm_bytes\":\"33554432\"}}"
        );
    }

    #[test]
    fn deploy_payload_json_supports_public_read_tuple() {
        let payload = circle_deploy_payload_json(None, ReadMode::Public).unwrap();
        assert_eq!(
            payload,
            "{\"runtime\":\"wasm_v1\",\"privacy_class\":\"public\",\"browser_mode\":\"gateway_allowed\",\"resource_mode\":\"public_resources\",\"code_b64\":null,\"policy_hash\":null,\"members_root\":null,\"export_policy\":null,\"limits\":{\"max_stable_bytes\":\"33554432\",\"max_assets_bytes\":\"33554432\",\"max_inline_value\":\"65536\",\"max_wasm_bytes\":\"33554432\"}}"
        );
    }

    #[test]
    fn deploy_payload_json_can_inline_wasm_code() {
        let payload = circle_deploy_payload_json(Some("QUJD"), ReadMode::Sealed).unwrap();
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
}
