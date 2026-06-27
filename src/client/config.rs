use super::error::{ClientError, ClientErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

const DEFAULT_CONFIG_JSON: &str = include_str!("../../config/defaults.json");

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub wallet: Option<String>,
    pub rpc: Option<String>,
    pub explorer: Option<String>,
    pub network: Option<String>,
    #[serde(default)]
    pub networks: BTreeMap<String, NetworkConfig>,
    #[serde(default, alias = "default_target")]
    pub default_database: Option<String>,
    #[serde(default, alias = "aliases")]
    pub databases: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkConfig {
    pub rpc: Option<String>,
    pub explorer: Option<String>,
}

impl Config {
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
}

pub fn config_path() -> Result<PathBuf> {
    if let Ok(path) = env::var("OCTRA_SQLITE_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    let home = dirs::home_dir().ok_or_else(|| {
        ClientError::with_kind(ClientErrorKind::Config, "could not locate home directory")
    })?;
    Ok(home.join(".octra").join("sqlite.json"))
}

pub fn load_config() -> Result<Config> {
    let defaults = bundled_default_config()?;
    let path = config_path()?;
    if !path.exists() {
        return Ok(defaults);
    }
    let text = fs::read_to_string(&path).map_err(|error| {
        ClientError::with_kind(
            ClientErrorKind::Io,
            format!("reading {}: {error}", path.display()),
        )
    })?;
    let user_config = serde_json::from_str(&text).map_err(|error| {
        ClientError::with_kind(
            ClientErrorKind::Config,
            format!("parsing {}: {error}", path.display()),
        )
    })?;
    Ok(merge_config(defaults, user_config))
}

pub fn write_config(config: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(config)? + "\n")?;
    Ok(())
}

fn bundled_default_config() -> Result<Config> {
    serde_json::from_str(DEFAULT_CONFIG_JSON).map_err(|error| {
        ClientError::with_kind(
            ClientErrorKind::Config,
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
    defaults
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_legacy_names_and_writes_database_names() {
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
    }

    #[test]
    fn user_config_overlays_bundled_defaults() {
        let defaults: Config = serde_json::from_str(
            r#"{"rpc":"http://default","network":"devnet","default_database":"remilia","databases":{"remilia":"oct://devnet/octA"}}"#,
        )
        .unwrap();
        let user: Config = serde_json::from_str(
            r#"{"rpc":"http://custom","default_database":"organization","databases":{"organization":"oct://devnet/octB"}}"#,
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
}
