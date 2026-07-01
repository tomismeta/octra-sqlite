use super::error::{ClientError, ClientErrorKind, Result};
#[cfg(feature = "cli")]
use crate::protocol::base58;
use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::SigningKey;
use serde::Deserialize;
#[cfg(feature = "cli")]
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

#[derive(Deserialize, Default)]
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

#[derive(Debug, PartialEq, Eq)]
#[cfg(feature = "cli")]
pub(crate) struct WalletMaterial {
    pub(crate) address: String,
    pub(crate) private_key_b64: String,
    pub(crate) public_key_b64: String,
}

#[cfg(feature = "cli")]
impl Drop for WalletMaterial {
    fn drop(&mut self) {
        self.private_key_b64.zeroize();
    }
}

pub(super) fn load_wallet(path: Option<&Path>) -> Result<WalletFile> {
    match path {
        Some(path) => {
            let mut text = fs::read_to_string(path).map_err(|error| {
                ClientError::with_kind(
                    ClientErrorKind::Io,
                    format!("reading wallet {}: {error}", path.display()),
                )
            })?;
            let parsed = serde_json::from_str(&text).map_err(|error| {
                ClientError::with_kind(
                    ClientErrorKind::Wallet,
                    format!("parsing wallet {}: {error}", path.display()),
                )
            });
            text.zeroize();
            Ok(parsed?)
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

#[cfg(feature = "cli")]
pub(crate) fn wallet_file_material(path: &Path) -> Result<WalletMaterial> {
    let wallet = load_wallet(Some(path))?;
    let supplied_address = first_string(&[wallet.addr, wallet.address]).ok_or_else(|| {
        ClientError::with_kind(
            ClientErrorKind::Wallet,
            format!("wallet {} is missing address/addr", path.display()),
        )
    })?;
    let mut private_key = first_secret_string([
        wallet.priv_field,
        wallet.priv_,
        wallet.private_key,
        wallet.private_key_b64,
    ])
    .ok_or_else(|| {
        ClientError::with_kind(
            ClientErrorKind::Wallet,
            format!("wallet {} is missing private_key_b64/priv", path.display()),
        )
    })?;
    let supplied_public_key = first_string(&[
        wallet.pub_field,
        wallet.pub_,
        wallet.public_key,
        wallet.public_key_b64,
    ]);
    let material = wallet_material_from_private_key(&private_key, supplied_public_key)?;
    private_key.zeroize();
    if material.address != supplied_address {
        return Err(ClientError::with_kind(
            ClientErrorKind::Wallet,
            format!(
                "wallet address {} does not match private key-derived address {}",
                supplied_address, material.address
            ),
        ));
    }
    Ok(material)
}

#[cfg(feature = "cli")]
pub(crate) fn wallet_material_from_private_key(
    private_key: &str,
    public_key: Option<String>,
) -> Result<WalletMaterial> {
    let key = signing_key_from_text(private_key)?;
    let public_key_bytes = key.verifying_key().to_bytes();
    let public_key_b64 = match public_key {
        Some(text) => normalized_public_key_b64(&text, &public_key_bytes)?,
        None => general_purpose::STANDARD.encode(public_key_bytes),
    };
    Ok(WalletMaterial {
        address: address_from_public_key(&public_key_bytes),
        private_key_b64: general_purpose::STANDARD.encode(key.to_bytes()),
        public_key_b64,
    })
}

pub(super) fn signing_key_from_text(text: &str) -> Result<SigningKey> {
    let cleaned = clean_key_text(text);
    let mut raw = decode_key_text(&cleaned).ok_or_else(|| {
        ClientError::with_kind(ClientErrorKind::Wallet, "private key must be base64 or hex")
    })?;
    if raw.len() != 32 && raw.len() != 64 {
        raw.zeroize();
        return Err(ClientError::with_kind(
            ClientErrorKind::Wallet,
            "private key must decode to a 32-byte seed or 64-byte keypair",
        ));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&raw[..32]);
    let signing_key = SigningKey::from_bytes(&seed);
    if raw.len() == 64 && raw[32..] != signing_key.verifying_key().to_bytes() {
        seed.zeroize();
        raw.zeroize();
        return Err(ClientError::with_kind(
            ClientErrorKind::Wallet,
            "64-byte private key public half does not match seed",
        ));
    }
    seed.zeroize();
    raw.zeroize();
    Ok(signing_key)
}

pub(super) fn normalized_public_key_b64(text: &str, expected: &[u8; 32]) -> Result<String> {
    let cleaned = clean_key_text(text);
    let mut raw = decode_key_text(&cleaned).ok_or_else(|| {
        ClientError::with_kind(ClientErrorKind::Wallet, "public key must be base64 or hex")
    })?;
    if raw.len() != 32 {
        raw.zeroize();
        return Err(ClientError::with_kind(
            ClientErrorKind::Wallet,
            "public key must decode to 32 bytes",
        ));
    }
    if raw.as_slice() != expected {
        raw.zeroize();
        return Err(ClientError::with_kind(
            ClientErrorKind::Wallet,
            "wallet public key does not match private key",
        ));
    }
    let public_key = general_purpose::STANDARD.encode(&raw);
    raw.zeroize();
    Ok(public_key)
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

#[cfg(feature = "cli")]
fn first_secret_string(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let mut selected = None;
    for mut value in values.into_iter().flatten() {
        if value.is_empty() {
            value.zeroize();
            continue;
        }
        if selected.is_none() {
            selected = Some(value);
        } else {
            value.zeroize();
        }
    }
    selected
}

fn clean_key_text(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn decode_key_text(cleaned: &str) -> Option<Vec<u8>> {
    let looks_hex =
        cleaned.len().is_multiple_of(2) && cleaned.as_bytes().iter().all(|b| b.is_ascii_hexdigit());
    if looks_hex {
        return hex::decode(cleaned).ok();
    }
    general_purpose::STANDARD.decode(cleaned).ok()
}

#[cfg(feature = "cli")]
fn address_from_public_key(public_key: &[u8; 32]) -> String {
    let digest = Sha256::digest(public_key);
    let mut encoded = base58::encode(&digest);
    while encoded.len() < 44 {
        encoded.insert(0, '1');
    }
    format!("oct{encoded}")
}

#[cfg(all(test, feature = "cli"))]
mod tests {
    use super::*;

    #[test]
    fn wallet_material_derives_octra_address() {
        let material =
            wallet_material_from_private_key("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", None)
                .unwrap();
        assert!(material.address.starts_with("oct"));
        assert_eq!(material.address.len(), 47);
        assert_eq!(
            material.private_key_b64,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        );
        assert!(!material.public_key_b64.is_empty());
    }

    #[test]
    fn wallet_material_strips_private_key_whitespace() {
        let material = wallet_material_from_private_key(
            "AAAA AAAA\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            None,
        )
        .unwrap();
        assert_eq!(material.address.len(), 47);
    }
}
