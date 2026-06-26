use super::error::{ClientError, ClientErrorKind, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
pub(super) struct WalletFile {
    pub(super) addr: Option<String>,
    pub(super) address: Option<String>,
    pub(super) priv_: Option<String>,
    #[serde(rename = "priv")]
    pub(super) priv_field: Option<String>,
    pub(super) private_key: Option<String>,
    pub(super) private_key_b64: Option<String>,
    pub(super) pub_: Option<String>,
    #[serde(rename = "pub")]
    pub(super) pub_field: Option<String>,
    pub(super) public_key: Option<String>,
    pub(super) public_key_b64: Option<String>,
    pub(super) rpc: Option<String>,
}

pub(super) fn load_wallet(path: Option<&Path>) -> Result<WalletFile> {
    match path {
        Some(path) => {
            let text = fs::read_to_string(path).map_err(|error| {
                ClientError::with_kind(
                    ClientErrorKind::Io,
                    format!("reading wallet {}: {error}", path.display()),
                )
            })?;
            Ok(serde_json::from_str(&text).map_err(|error| {
                ClientError::with_kind(
                    ClientErrorKind::Wallet,
                    format!("parsing wallet {}: {error}", path.display()),
                )
            })?)
        }
        None => Ok(WalletFile::default()),
    }
}

pub fn discover_wallet_path() -> Option<PathBuf> {
    wallet_candidates().into_iter().find(|path| path.is_file())
}

pub fn wallet_caller(path: Option<&Path>, caller: Option<&str>) -> Result<Option<String>> {
    let wallet = load_wallet(path)?;
    Ok(first_string(&[
        caller.map(str::to_string),
        wallet.addr,
        wallet.address,
        env::var("OCTRA_CALLER").ok(),
    ]))
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
