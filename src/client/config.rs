use super::error::{Error, ErrorKind, Result};
use crate::private_file::atomic_replace;
use crate::protocol::target::{DatabaseTarget, ReadMode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_CONFIG_JSON: &str = include_str!("../../config/defaults.json");

/// Local octra-sqlite configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Active wallet JSON path.
    pub wallet: Option<String>,
    /// Active network RPC URL.
    pub rpc: Option<String>,
    /// Active network explorer base URL.
    pub explorer: Option<String>,
    /// Active network name.
    pub network: Option<String>,
    /// Named network profiles.
    #[serde(default)]
    pub networks: BTreeMap<String, NetworkConfig>,
    /// Saved database name selected when no target is given.
    #[serde(default)]
    pub default_database: Option<String>,
    /// Saved database names mapped to `oct://` URIs.
    #[serde(default)]
    pub databases: BTreeMap<String, String>,
    /// Deployment metadata keyed by saved database name.
    #[serde(default)]
    pub database_metadata: BTreeMap<String, DatabaseMetadata>,
}

/// Per-network RPC and explorer profile.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkConfig {
    /// Octra RPC URL for this network.
    pub rpc: Option<String>,
    /// Explorer base URL for this network.
    pub explorer: Option<String>,
}

/// Saved database metadata written by `octra-sqlite new`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DatabaseMetadata {
    /// Canonical `oct://` database URI.
    pub uri: String,
    /// Octra network name.
    pub network: String,
    /// Circle ID.
    pub circle: String,
    /// Saved read-mode preference.
    #[serde(default = "default_read_mode")]
    pub read_mode: ReadMode,
    /// Octra deployment privacy class.
    #[serde(default = "default_privacy_class")]
    pub privacy_class: String,
    /// Octra browser access mode.
    #[serde(default = "default_browser_mode")]
    pub browser_mode: String,
    /// Octra resource access mode.
    #[serde(default = "default_resource_mode")]
    pub resource_mode: String,
    /// Circle owner address at creation.
    pub owner: String,
    /// Owner public key personalized into the Circle WASM.
    pub owner_pubkey: String,
    /// Database identity personalized into OSW1.
    pub db_id: String,
    /// Personalized deployed WASM SHA-256.
    pub code_hash: String,
    /// Deployed WASM byte length.
    pub code_bytes: usize,
    /// Circle creation transaction hash when known.
    pub create_tx: Option<String>,
    /// Program update transaction hash when one was required.
    #[serde(default)]
    pub program_update_tx: Option<String>,
}

fn default_privacy_class() -> String {
    "sealed".to_string()
}

fn default_read_mode() -> ReadMode {
    ReadMode::Sealed
}

fn default_browser_mode() -> String {
    "native_sealed".to_string()
}

fn default_resource_mode() -> String {
    "sealed_read".to_string()
}

impl Config {
    /// Resolve the RPC URL for a network, preferring active overrides.
    pub fn rpc_for_network(&self, network: &str) -> Option<String> {
        if self.network.as_deref() == Some(network) {
            return self
                .rpc
                .clone()
                .or_else(|| self.networks.get(network)?.rpc.clone());
        }
        self.networks
            .get(network)
            .and_then(|profile| profile.rpc.clone())
    }

    /// Resolve the explorer URL for a network, preferring active overrides.
    pub fn explorer_for_network(&self, network: &str) -> Option<String> {
        if self.network.as_deref() == Some(network) {
            return self
                .explorer
                .clone()
                .or_else(|| self.networks.get(network)?.explorer.clone());
        }
        self.networks
            .get(network)
            .and_then(|profile| profile.explorer.clone())
    }

    /// Copy the active network profile into the legacy active URL fields.
    pub fn apply_active_network_profile(&mut self) {
        let Some(network) = self.network.as_deref() else {
            return;
        };
        let Some(profile) = self.networks.get(network) else {
            return;
        };
        if let Some(rpc) = &profile.rpc {
            self.rpc = Some(rpc.clone());
        }
        if let Some(explorer) = &profile.explorer {
            self.explorer = Some(explorer.clone());
        }
    }

    /// Find saved deployment metadata for a requested and resolved target.
    pub fn metadata_for_target(
        &self,
        requested: &str,
        target: &DatabaseTarget,
    ) -> Option<&DatabaseMetadata> {
        self.database_metadata.get(requested).or_else(|| {
            self.database_metadata.values().find(|metadata| {
                metadata.uri == requested
                    || metadata.uri == target.raw
                    || (metadata.network == target.network && metadata.circle == target.circle)
            })
        })
    }
}

/// Return the active config path, including `OCTRA_SQLITE_CONFIG` overrides.
pub fn config_path() -> Result<PathBuf> {
    if let Ok(path) = env::var("OCTRA_SQLITE_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    let home = dirs::home_dir()
        .ok_or_else(|| Error::with_kind(ErrorKind::Config, "could not locate home directory"))?;
    Ok(home.join(".octra").join("sqlite.json"))
}

/// Load bundled defaults overlaid with the active local config.
pub fn load_config() -> Result<Config> {
    let path = config_path()?;
    load_config_at(&path)
}

fn load_config_at(path: &Path) -> Result<Config> {
    let defaults = bundled_default_config()?;
    if !path.try_exists().map_err(|error| {
        Error::with_kind(
            ErrorKind::Io,
            format!("checking {}: {error}", path.display()),
        )
    })? {
        return Ok(defaults);
    }
    let text = fs::read_to_string(path).map_err(|error| {
        Error::with_kind(
            ErrorKind::Io,
            format!("reading {}: {error}", path.display()),
        )
    })?;
    let user_config = serde_json::from_str(&text).map_err(|error| {
        Error::with_kind(
            ErrorKind::Config,
            format!("parsing {}: {error}", path.display()),
        )
    })?;
    Ok(merge_config(defaults, user_config))
}

/// Atomically write local config with owner-only permissions where supported.
pub fn write_config(config: &Config) -> Result<()> {
    let path = config_path()?;
    write_config_at(&path, config)
}

fn write_config_at(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let contents = serde_json::to_string_pretty(config)? + "\n";
    atomic_replace(path, contents.as_bytes())?;
    Ok(())
}

fn bundled_default_config() -> Result<Config> {
    serde_json::from_str(DEFAULT_CONFIG_JSON).map_err(|error| {
        Error::with_kind(
            ErrorKind::Config,
            format!("parsing bundled default config: {error}"),
        )
    })
}

fn merge_config(mut defaults: Config, user: Config) -> Config {
    defaults.wallet = user.wallet.or(defaults.wallet);
    defaults.network = user.network.or(defaults.network);
    defaults.networks.extend(user.networks);
    defaults.rpc = user
        .rpc
        .or_else(|| {
            defaults
                .network
                .as_deref()
                .and_then(|network| defaults.networks.get(network)?.rpc.clone())
        })
        .or(defaults.rpc);
    defaults.explorer = user
        .explorer
        .or_else(|| {
            defaults
                .network
                .as_deref()
                .and_then(|network| defaults.networks.get(network)?.explorer.clone())
        })
        .or(defaults.explorer);
    defaults.default_database = user.default_database.or(defaults.default_database);
    defaults.databases.extend(user.databases);
    defaults.database_metadata.extend(user.database_metadata);
    defaults
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_database_names() {
        let config: Config = serde_json::from_str(
            r#"{"default_database":"organization","databases":{"organization":"oct://devnet/octABC"}}"#,
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
        assert!(written.contains("database_metadata"));
    }

    #[test]
    fn bundled_defaults_are_product_clean() {
        let config = bundled_default_config().unwrap();
        assert_eq!(config.network.as_deref(), Some("devnet"));
        assert!(config.default_database.is_none());
        assert_eq!(
            config.rpc.as_deref(),
            Some("https://devnet.octrascan.io/rpc")
        );
        assert_eq!(
            config
                .networks
                .get("devnet")
                .and_then(|network| network.rpc.as_deref()),
            Some("https://devnet.octrascan.io/rpc")
        );
        assert_eq!(
            config.explorer.as_deref(),
            Some("https://devnet.octrascan.io")
        );
        assert_eq!(
            config
                .networks
                .get("mainnet")
                .and_then(|network| network.rpc.as_deref()),
            Some("https://octra.network/rpc")
        );
        assert_eq!(
            config
                .networks
                .get("mainnet")
                .and_then(|network| network.explorer.as_deref()),
            Some("https://octrascan.io")
        );
        assert!(config.databases.is_empty());
        assert!(config.database_metadata.is_empty());
    }

    #[test]
    fn user_config_overlays_bundled_defaults() {
        let defaults: Config = serde_json::from_str(
            r#"{"rpc":"http://default","network":"devnet","default_database":"remilia","databases":{"remilia":"oct://devnet/octA"}}"#,
        )
        .unwrap();
        let user: Config = serde_json::from_str(
            r#"{"rpc":"http://custom","default_database":"organization","databases":{"organization":"oct://devnet/octB"},"database_metadata":{"organization":{"uri":"oct://devnet/octB","network":"devnet","circle":"octB","owner":"octOwner","owner_pubkey":"aa","db_id":"bb","code_hash":"cc","code_bytes":123,"create_tx":"tx"}}}"#,
        )
        .unwrap();
        let merged = merge_config(defaults, user);
        assert_eq!(merged.rpc.as_deref(), Some("http://custom"));
        assert_eq!(merged.network.as_deref(), Some("devnet"));
        assert_eq!(merged.default_database.as_deref(), Some("organization"));
        assert_eq!(
            merged.databases.get("remilia").map(String::as_str),
            Some("oct://devnet/octA")
        );
        assert_eq!(
            merged.databases.get("organization").map(String::as_str),
            Some("oct://devnet/octB")
        );
        assert_eq!(
            merged
                .database_metadata
                .get("organization")
                .map(|metadata| metadata.code_hash.as_str()),
            Some("cc")
        );
        assert_eq!(
            merged
                .database_metadata
                .get("organization")
                .and_then(|metadata| metadata.program_update_tx.as_deref()),
            None
        );
    }

    #[test]
    fn network_profiles_supply_active_urls() {
        let defaults: Config = serde_json::from_str(
            r#"{
                "rpc":"http://devnet",
                "explorer":"https://devnet",
                "network":"devnet",
                "networks":{
                    "devnet":{"rpc":"http://devnet","explorer":"https://devnet"},
                    "mainnet":{"rpc":"https://octra.network/rpc","explorer":"https://octrascan.io"}
                }
            }"#,
        )
        .unwrap();
        let user: Config = serde_json::from_str(r#"{"network":"mainnet"}"#).unwrap();
        let merged = merge_config(defaults, user);
        assert_eq!(merged.network.as_deref(), Some("mainnet"));
        assert_eq!(merged.rpc.as_deref(), Some("https://octra.network/rpc"));
        assert_eq!(merged.explorer.as_deref(), Some("https://octrascan.io"));
        assert_eq!(
            merged.rpc_for_network("devnet").as_deref(),
            Some("http://devnet")
        );
    }

    #[test]
    fn config_write_replaces_the_file_atomically() {
        let root = std::env::temp_dir().join(format!(
            "octra-sqlite-config-write-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("sqlite.json");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, "{\"network\":\"old\"}\n").unwrap();
        let config = Config {
            network: Some("devnet".to_string()),
            ..Config::default()
        };
        write_config_at(&path, &config).unwrap();
        let written: Config = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written.network.as_deref(), Some("devnet"));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_config_fails_closed() {
        let path = std::env::temp_dir().join(format!(
            "octra-sqlite-malformed-config-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, "{").unwrap();
        let error = load_config_at(&path).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Config);
        assert!(error.to_string().contains("parsing"));
        fs::remove_file(path).unwrap();
    }
}
