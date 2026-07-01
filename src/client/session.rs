use super::config::{load_config, Config};
use super::error::{ClientError, ClientErrorKind, Result};
use super::wallet::{
    discover_wallet_path, load_wallet, normalized_public_key_b64, signing_key_from_text,
};
use crate::protocol::target::{parse_database_target, DatabaseTarget};
use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zeroize::Zeroize;

#[derive(Clone, Default)]
pub struct SessionOptions {
    pub target: Option<String>,
    pub wallet: Option<PathBuf>,
    pub rpc: Option<String>,
    pub caller: Option<String>,
    pub private_key: Option<String>,
    pub public_key: Option<String>,
}

#[derive(Clone)]
pub struct Session {
    target: DatabaseTarget,
    wallet_path: Option<PathBuf>,
    rpc: String,
    rpc_override: bool,
    caller: String,
    signer: Arc<LocalSigner>,
}

struct LocalSigner {
    key: SigningKey,
    public_key_b64: String,
}

impl LocalSigner {
    fn from_private_key_text(private_key: &str, public_key: Option<String>) -> Result<Self> {
        let key = signing_key_from_text(private_key)?;
        let derived_public_key = key.verifying_key().to_bytes();
        let public_key_b64 = match public_key {
            Some(text) => normalized_public_key_b64(&text, &derived_public_key)?,
            None => general_purpose::STANDARD.encode(derived_public_key),
        };
        Ok(Self {
            key,
            public_key_b64,
        })
    }

    fn public_key_b64(&self) -> &str {
        &self.public_key_b64
    }

    fn intent_public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    fn sign_text_b64(&self, message: &str) -> String {
        general_purpose::STANDARD.encode(self.key.sign(message.as_bytes()).to_bytes())
    }

    fn sign_bytes_hex(&self, message: &[u8]) -> String {
        hex::encode(self.key.sign(message).to_bytes())
    }
}

impl Session {
    pub fn target(&self) -> &DatabaseTarget {
        &self.target
    }

    pub fn wallet_path(&self) -> Option<&Path> {
        self.wallet_path.as_deref()
    }

    pub fn rpc(&self) -> &str {
        &self.rpc
    }

    pub fn caller(&self) -> &str {
        &self.caller
    }

    pub fn public_key_b64(&self) -> &str {
        self.signer.public_key_b64()
    }

    pub fn with_database_target(&self, target: DatabaseTarget) -> Session {
        Session {
            target,
            wallet_path: self.wallet_path.clone(),
            rpc: self.rpc.clone(),
            rpc_override: self.rpc_override,
            caller: self.caller.clone(),
            signer: Arc::clone(&self.signer),
        }
    }

    pub fn open_database(&self, target: impl Into<String>) -> Result<Session> {
        let config = load_config().unwrap_or_default();
        let mut target = resolve_target(&target.into(), &config)?;
        if target.rpc.is_empty() {
            target.rpc = self.rpc.clone();
        }
        Ok(Session {
            rpc: open_database_rpc(&self.rpc, self.rpc_override, Some(target.rpc.clone())),
            target,
            wallet_path: self.wallet_path.clone(),
            rpc_override: self.rpc_override,
            caller: self.caller.clone(),
            signer: Arc::clone(&self.signer),
        })
    }

    pub fn intent_public_key(&self) -> Result<[u8; 32]> {
        Ok(self.signer.intent_public_key())
    }

    pub(crate) fn sign_view_auth_b64(&self, message: &str) -> String {
        self.signer.sign_text_b64(message)
    }

    pub(crate) fn sign_program_info_b64(&self, message: &str) -> String {
        self.signer.sign_text_b64(message)
    }

    pub(crate) fn sign_transaction_b64(&self, message: &str) -> String {
        self.signer.sign_text_b64(message)
    }

    pub(crate) fn sign_owner_write_hex(&self, message: &[u8]) -> String {
        self.signer.sign_bytes_hex(message)
    }
}

pub fn build_session(options: &SessionOptions) -> Result<Session> {
    let config = load_config().unwrap_or_default();
    let target_value = options
        .target
        .clone()
        .or_else(|| config.default_database.clone())
        .or_else(|| env::var("OCTRA_SQLITE_DATABASE").ok())
        .or_else(|| env::var("OCTRA_SQLITE_TARGET").ok())
        .or_else(|| env::var("OCTRA_CIRCLE_ID").ok())
        .ok_or_else(|| {
            ClientError::with_kind(
                ClientErrorKind::Config,
                "no database supplied and no default database is configured",
            )
        })?;
    let target = resolve_target(&target_value, &config)?;
    build_session_for_target(options, &config, target)
}

pub fn build_control_session(options: &SessionOptions, network: &str) -> Result<Session> {
    let config = load_config().unwrap_or_default();
    let target = DatabaseTarget {
        raw: format!("oct://{network}"),
        network: network.to_string(),
        circle: String::new(),
        rpc: config.rpc_for_network(network).unwrap_or_default(),
    };
    build_session_for_target(options, &config, target)
}

pub fn resolve_wallet_path(options: &SessionOptions, config: &Config) -> Option<PathBuf> {
    options
        .wallet
        .clone()
        .or_else(|| env::var("OCTRA_WALLET").ok().map(PathBuf::from))
        .or_else(|| config.wallet.as_ref().map(PathBuf::from))
        .or_else(discover_wallet_path)
}

fn resolve_target(value: &str, config: &Config) -> Result<DatabaseTarget> {
    if let Some(database) = config.databases.get(value) {
        return resolve_target(database, config);
    }
    let mut target = parse_database_target(value, config.network.as_deref(), None)?;
    if target.rpc.is_empty() {
        target.rpc = config.rpc_for_network(&target.network).unwrap_or_default();
    }
    Ok(target)
}

fn build_session_for_target(
    options: &SessionOptions,
    config: &Config,
    mut target: DatabaseTarget,
) -> Result<Session> {
    let explicit_rpc = first_string([options.rpc.clone(), env::var("OCTRA_RPC_URL").ok()]);
    if let Some(rpc) = explicit_rpc.clone() {
        target.rpc = rpc;
    }
    let rpc_override = explicit_rpc.is_some();
    let wallet_path = resolve_wallet_path(options, config);
    let wallet = load_wallet(wallet_path.as_deref())?;
    let wallet_rpc = wallet.rpc;
    let rpc = choose_session_rpc(
        explicit_rpc,
        Some(target.rpc.clone()),
        config.rpc.clone(),
        wallet_rpc,
    )
    .ok_or_else(|| {
        ClientError::with_kind(
            ClientErrorKind::Config,
            "RPC is required; run octra-sqlite setup, pass --rpc, or set OCTRA_RPC_URL",
        )
    })?;
    let caller = first_string([
        options.caller.clone(),
        wallet.addr,
        wallet.address,
        env::var("OCTRA_CALLER").ok(),
    ])
    .ok_or_else(|| {
        ClientError::with_kind(
            ClientErrorKind::Wallet,
            "caller wallet address is required; configure a wallet, pass --caller, or set OCTRA_CALLER",
        )
    })?;
    let mut private_key = first_secret_string([
        options.private_key.clone(),
        wallet.priv_field,
        wallet.priv_,
        wallet.private_key,
        wallet.private_key_b64,
        env::var("OCTRA_PRIVATE_KEY_B64").ok(),
    ])
    .ok_or_else(|| {
        ClientError::with_kind(
            ClientErrorKind::Wallet,
            "wallet private key is required; pass --wallet or OCTRA_PRIVATE_KEY_B64",
        )
    })?;
    let supplied_public_key = first_string([
        options.public_key.clone(),
        wallet.pub_field,
        wallet.pub_,
        wallet.public_key,
        wallet.public_key_b64,
        env::var("OCTRA_PUBLIC_KEY_B64").ok(),
    ]);
    let signer = LocalSigner::from_private_key_text(&private_key, supplied_public_key);
    private_key.zeroize();
    let signer = Arc::new(signer?);
    Ok(Session {
        target,
        wallet_path,
        rpc,
        rpc_override,
        caller,
        signer,
    })
}

fn first_string(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .find_map(|value| value.filter(|v| !v.is_empty()))
}

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

fn open_database_rpc(current_rpc: &str, rpc_override: bool, target_rpc: Option<String>) -> String {
    if rpc_override {
        return current_rpc.to_string();
    }
    first_string([target_rpc, Some(current_rpc.to_string())]).unwrap()
}

fn choose_session_rpc(
    explicit_rpc: Option<String>,
    target_rpc: Option<String>,
    config_rpc: Option<String>,
    wallet_rpc: Option<String>,
) -> Option<String> {
    first_string([explicit_rpc, target_rpc, config_rpc, wallet_rpc])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_network_rpc_wins_over_wallet_rpc() {
        assert_eq!(
            choose_session_rpc(
                None,
                Some("https://devnet.octrascan.io/rpc".to_string()),
                Some("https://config.example/rpc".to_string()),
                Some("http://wallet.example/rpc".to_string()),
            )
            .as_deref(),
            Some("https://devnet.octrascan.io/rpc")
        );
    }

    #[test]
    fn explicit_rpc_wins_over_target_network_rpc() {
        assert_eq!(
            choose_session_rpc(
                Some("https://override.example/rpc".to_string()),
                Some("https://devnet.octrascan.io/rpc".to_string()),
                Some("https://config.example/rpc".to_string()),
                Some("http://wallet.example/rpc".to_string()),
            )
            .as_deref(),
            Some("https://override.example/rpc")
        );
    }

    #[test]
    fn wallet_rpc_is_only_a_fallback() {
        assert_eq!(
            choose_session_rpc(
                None,
                Some(String::new()),
                None,
                Some("http://wallet.example/rpc".to_string()),
            )
            .as_deref(),
            Some("http://wallet.example/rpc")
        );
    }

    #[test]
    fn open_database_rpc_uses_target_network_unless_rpc_was_explicit() {
        assert_eq!(
            open_database_rpc(
                "https://devnet.octrascan.io/rpc",
                false,
                Some("https://octra.network/rpc".to_string()),
            ),
            "https://octra.network/rpc"
        );
        assert_eq!(
            open_database_rpc(
                "http://127.0.0.1:8080/rpc",
                true,
                Some("https://octra.network/rpc".to_string()),
            ),
            "http://127.0.0.1:8080/rpc"
        );
    }

    #[test]
    fn supplied_public_key_must_match_private_key() {
        let error = match build_session(&SessionOptions {
            target: Some("oct://devnet/octABC".to_string()),
            rpc: Some("mock://rpc".to_string()),
            caller: Some("octCaller".to_string()),
            private_key: Some(
                "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
            ),
            public_key: Some(general_purpose::STANDARD.encode([2u8; 32])),
            ..SessionOptions::default()
        }) {
            Ok(_) => panic!("mismatched public key should fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), ClientErrorKind::Wallet);
        assert!(error
            .to_string()
            .contains("wallet public key does not match private key"));
    }

    #[test]
    fn accepts_explicit_64_byte_keypair_form() {
        let seed = [3u8; 32];
        let key = SigningKey::from_bytes(&seed);
        let public_key = key.verifying_key().to_bytes();
        let mut keypair = Vec::from(seed);
        keypair.extend_from_slice(&public_key);
        let session = build_session(&SessionOptions {
            target: Some("oct://devnet/octABC".to_string()),
            rpc: Some("mock://rpc".to_string()),
            caller: Some("octCaller".to_string()),
            private_key: Some(hex::encode(keypair)),
            public_key: Some(general_purpose::STANDARD.encode(public_key)),
            ..SessionOptions::default()
        })
        .unwrap();
        assert_eq!(
            session.public_key_b64(),
            general_purpose::STANDARD.encode(public_key)
        );
    }

    #[test]
    fn rejects_private_keys_with_ambiguous_length() {
        let error = signing_key_from_text("0102").unwrap_err();
        assert_eq!(error.kind(), ClientErrorKind::Wallet);
        assert!(error
            .to_string()
            .contains("32-byte seed or 64-byte keypair"));
    }
}
