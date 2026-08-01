//! SQLite-shaped CLI orchestration for humans and automation.
//!
//! [`run`] and [`run_with_exit_code`] execute the command-line interface.
//! [`error_code`] returns the stable automation code for a returned error.

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose};
use clap::{Args, Parser, Subcommand, ValueEnum};
mod catalog;
mod databases;
mod deployment;
mod error;
mod inspection;
mod onboarding;
mod output;
mod portability;
mod shell;
mod sql;
mod upgrade;
use crate::{
    client::{
        AuthInfo, ClientOptions, Config, Database, DatabaseMetadata, Error, ErrorKind,
        RpcTraceMode, config_path, load_config,
        raw::{
            Session, WalletMaterial, auth_info,
            build_control_session as client_build_control_session,
            build_session as client_build_session, circle_info, discover_wallet_path, exec_sql,
            next_nonce, program_info, query_typed, query_typed_traced,
            resolve_database_target as client_resolve_database_target,
            resolve_wallet_path as client_resolve_wallet_path, submit_tx, transaction,
            transactions_by_address, view, wait_for_transaction, wallet_caller,
            wallet_file_material, wallet_material_from_private_key,
        },
        write_config,
    },
    protocol::{
        base58,
        target::{DatabaseTarget as Target, ReadMode, parse_database_target},
        tx::Tx,
    },
};
use catalog::*;
use databases::*;
use deployment::*;
use error::{
    auth_error, auth_info_error, coded_error, target_error, wallet_error, with_fallback_code,
};
use inspection::*;
use onboarding::*;
use output::{
    OutputMode, dim, format_exec_result, format_field, format_json, format_result,
    format_status_line, hyperlink, print_exec_result, print_json, print_result, strong,
    value_to_string, write_text,
};
use portability::{
    MAX_SQL_TEXT_BYTES, SQL_BATCH_TARGET_BYTES, SqlBatchProgress, SqlScriptExecution,
    SqlScriptPlan, backup_database, ensure_sql_text_fits,
    execute_sql_script_with_bootstrap_owner_progress, execute_sql_script_with_owner_auth_progress,
    execute_sql_script_with_progress, plan_sql_script, run_local_sqlite_integrity,
    submit_sql_script_no_wait,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use shell::{run_dot_command, run_shell};
use sql::*;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::{Zeroize, Zeroizing};

const DEFAULT_WASM_REL: &str = "circle/wasm/octra_sqlite_circle.wasm";
const BUILD_WASM_SCRIPT_REL: &str = "scripts/build-wasm.sh";
const RELEASE_MANIFEST_REL: &str = "release/octra-sqlite-0.6.3.json";
const EMBEDDED_WASM_BYTES: &[u8] = include_bytes!("../../circle/wasm/octra_sqlite_circle.wasm");
const EMBEDDED_RELEASE_MANIFEST: &str = include_str!("../../release/octra-sqlite-0.6.3.json");
const OWNER_PUBKEY_PLACEHOLDER: &[u8; 32] = b"OSQL_OWNER_PUBKEY_V1_PLACEHOLDER";
const DB_ID_PLACEHOLDER: &[u8; 32] = b"OSQL_DATABASE_ID_V1_PLACEHOLDER0";
const EXPECTED_WASM_SHA256: &str =
    "8fe0dad1a4bb4fcfc7afab626a58eda45edeac3b25607f130b201997698d8bcf";
const EXPECTED_WASM_BYTES: usize = 611_677;
const CREATE_DATABASE_COMMAND: &str = "octra-sqlite new";
const SQLITE_VERSION: &str = "3.53.4";
const MAX_RESULT_ROWS: usize = 512;
const MAX_RESPONSE_BYTES: usize = 65_526;
const MAX_DB_PAGES: usize = 8_069;
const SQLITE_PAGE_BYTES: usize = 4_096;
const MAX_DB_FILE_BYTES: usize = 33_050_624;
const STABLE_STORAGE_LIMIT_BYTES: usize = 33_554_432;
const MAX_DIRTY_PAGES_PER_EXEC: usize = 1_024;
const MAX_QUERY_VDBE_STEPS: usize = 5_000_000;
const MAX_EXEC_VDBE_STEPS: usize = 25_000_000;
const OFFICIAL_WALLET_GENERATOR_URL: &str = "https://wallet.octra.org";
const UPGRADE_BUNDLE_SCHEMA: &str = "octra-sqlite.upgrade.bundle.v1";

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
    /// Safely upgrade an existing octra-sqlite Circle to the bundled engine.
    Upgrade(UpgradeArgs),
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
}

#[derive(Subcommand)]
#[command(after_long_help = "\
Examples:
  octra-sqlite database list
  octra-sqlite database info art
  octra-sqlite database default art
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
    Default { name: String },
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
struct UpgradeArgs {
    /// Database to upgrade, or `rollback` to restore a bundle's previous program.
    #[arg(value_name = "DATABASE|rollback")]
    target: Option<String>,
    /// Bundle directory when using `upgrade rollback`.
    #[arg(value_name = "BUNDLE")]
    rollback_bundle: Option<PathBuf>,
    /// Run preflight and print the plan without writing backup files or updating the Circle.
    #[arg(long)]
    dry_run: bool,
    /// Directory where the upgrade bundle should be written.
    #[arg(long)]
    backup_dir: Option<PathBuf>,
    /// Skip the local SQLite backup before upgrade.
    #[arg(long)]
    skip_backup: bool,
    /// Fail if local sqlite3 integrity_check cannot be run.
    #[arg(long)]
    require_integrity: bool,
    /// Run an owner-signed write smoke after the program update.
    #[arg(long)]
    write_smoke: bool,
    /// UNSAFE: continue without rollback bytes if the previous live WASM cannot be recovered.
    #[arg(long)]
    unsafe_no_rollback: bool,
    /// Previous personalized or base WASM to use when rollback bytes cannot be recovered from chain history.
    #[arg(long, value_name = "FILE")]
    previous_wasm: Option<PathBuf>,
    /// For rollback only: allow redeploying the old engine after post-upgrade writes.
    #[arg(long)]
    force_after_writes: bool,
    /// OU budget for Circle program update.
    #[arg(long, default_value = "200000")]
    ou: String,
    /// Do not prompt for confirmation.
    #[arg(long)]
    yes: bool,
    /// Print a stable JSON summary.
    #[arg(long)]
    json: bool,
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

/// Parse process arguments and run one CLI command.
pub fn run() -> Result<()> {
    let code = run_with_exit_code()?;
    if code == 0 {
        Ok(())
    } else {
        bail!("command exited with status {code}")
    }
}

/// Parse process arguments, run one command, and return its requested exit code.
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
        Commands::Upgrade(args) => upgrade::cmd_upgrade(args).map(|_| 0),
        Commands::Status(args) => cmd_status(args, "status"),
        Commands::Config(args) => cmd_config(args).map(|_| 0),
        Commands::Wallet { command } => cmd_wallet(command).map(|_| 0),
        Commands::Deploy(args) => cmd_deploy(args).map(|_| 0),
    }
}

pub use error::error_code;

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
        "upgrade",
        "status",
        "config",
        "wallet",
        "deploy",
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

pub(super) fn print_field(label: &str, detail: impl AsRef<str>) {
    print!("{}", format_field(label, detail));
}

fn print_title(title: &str) {
    println!("{}", strong(title));
    println!();
}

fn print_section(title: &str) {
    println!("{}", strong(title));
}

fn print_choice(number: usize, label: &str) {
    println!("  {}  {label}", dim(format!("{number}.")));
}

fn print_warning(detail: impl AsRef<str>) {
    println!("{} {}", strong("warning:"), dim(detail.as_ref()));
}

fn print_command(label: &str, command: impl AsRef<str>) {
    println!("{} {}", dim(format!("{label}:")), command.as_ref());
}

fn session_options(args: &TargetArgs) -> ClientOptions {
    ClientOptions {
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

fn explicit_target_allows_unsigned_read(args: &TargetArgs, config: &Config) -> bool {
    let Some(target) = args.target.as_deref() else {
        return false;
    };
    let Ok(target) = resolve_target(target, config) else {
        return false;
    };
    match target.read_mode {
        ReadMode::Public => true,
        ReadMode::Auto => client_build_session(&session_options(args))
            .ok()
            .and_then(|session| circle_info(&session).ok())
            .is_some_and(|info| circle_info_allows_unsigned_read(&info)),
        ReadMode::Sealed => false,
    }
}

fn circle_info_allows_unsigned_read(info: &Value) -> bool {
    info.get("privacy_class").and_then(Value::as_str) == Some("public")
        && info.get("browser_mode").and_then(Value::as_str) == Some("gateway_allowed")
        && info.get("resource_mode").and_then(Value::as_str) == Some("public_resources")
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
    print!("{} [{}]: ", dim(label), default);
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

fn prompt_choice_default(label: &str, default: &str, choices: &str) -> Result<String> {
    print!("{} [{}] {}: ", dim(label), default, dim(choices));
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
    print!("{}: ", dim(label));
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{label} is required");
    }
    Ok(trimmed.to_string())
}

fn prompt_path_no_default(label: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(prompt_required(label)?))
}

fn prompt_read_mode(default: ReadModeArg) -> Result<ReadModeArg> {
    let default_text = match default {
        ReadModeArg::Sealed => "sealed",
        ReadModeArg::Public => "public",
    };
    let value = prompt_choice_default("read mode", default_text, "(sealed/public)")?;
    match value.trim().to_ascii_lowercase().as_str() {
        "sealed" => Ok(ReadModeArg::Sealed),
        "public" => Ok(ReadModeArg::Public),
        _ => bail!("read mode must be sealed or public"),
    }
}

fn prompt_network(default: &str) -> Result<String> {
    let value = prompt_choice_default("network", default, "(devnet/mainnet)")?;
    match value.trim().to_ascii_lowercase().as_str() {
        "devnet" => Ok("devnet".to_string()),
        "mainnet" => Ok("mainnet".to_string()),
        _ => bail!("network must be devnet or mainnet"),
    }
}

fn prompt_yes_no(label: &str, default: bool) -> Result<bool> {
    let default_text = if default { "Y/n" } else { "y/N" };
    print!("{} [{}]: ", dim(label), default_text);
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
    Ok(client_resolve_database_target(value, config)?)
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
    if target.read_mode == ReadMode::Auto
        && let Some(metadata) = config.metadata_for_target(requested, target)
    {
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

#[cfg(test)]
mod tests;
