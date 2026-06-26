use super::config::{load_config, Config};
use super::error::{ClientError, ClientErrorKind, Result};
use super::wallet::{discover_wallet_path, load_wallet};
use crate::protocol::target::{parse_database_target, DatabaseTarget};
use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use std::env;
use std::path::{Path, PathBuf};

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
    caller: String,
    private_key: String,
    public_key: String,
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
        &self.public_key
    }

    pub fn with_database_target(&self, target: DatabaseTarget) -> Session {
        Session {
            target,
            wallet_path: self.wallet_path.clone(),
            rpc: self.rpc.clone(),
            caller: self.caller.clone(),
            private_key: self.private_key.clone(),
            public_key: self.public_key.clone(),
        }
    }

    pub fn open_database(&self, target: impl Into<String>) -> Result<Session> {
        build_session(&SessionOptions {
            target: Some(target.into()),
            wallet: self.wallet_path.clone(),
            rpc: Some(self.rpc.clone()),
            caller: Some(self.caller.clone()),
            private_key: Some(self.private_key.clone()),
            public_key: Some(self.public_key.clone()),
        })
    }

    pub fn intent_public_key(&self) -> Result<[u8; 32]> {
        Ok(signing_key_from_text(&self.private_key)?
            .verifying_key()
            .to_bytes())
    }

    pub(crate) fn sign_text_b64(&self, message: &str) -> Result<String> {
        let signing_key = signing_key_from_text(&self.private_key)?;
        Ok(general_purpose::STANDARD.encode(signing_key.sign(message.as_bytes()).to_bytes()))
    }

    pub(crate) fn sign_bytes_hex(&self, message: &[u8]) -> Result<String> {
        let signing_key = signing_key_from_text(&self.private_key)?;
        Ok(hex::encode(signing_key.sign(message).to_bytes()))
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
    let explicit_rpc = first_string(&[options.rpc.clone(), env::var("OCTRA_RPC_URL").ok()]);
    if let Some(rpc) = explicit_rpc.clone() {
        target.rpc = rpc;
    }
    let wallet_path = resolve_wallet_path(options, config);
    let wallet = load_wallet(wallet_path.as_deref())?;
    let rpc = choose_session_rpc(
        explicit_rpc,
        Some(target.rpc.clone()),
        config.rpc.clone(),
        wallet.rpc.clone(),
    )
    .ok_or_else(|| {
        ClientError::with_kind(
            ClientErrorKind::Config,
            "RPC is required; run octra-sqlite setup, pass --rpc, or set OCTRA_RPC_URL",
        )
    })?;
    let caller = first_string(&[
        options.caller.clone(),
        wallet.addr.clone(),
        wallet.address.clone(),
        env::var("OCTRA_CALLER").ok(),
    ])
    .ok_or_else(|| {
        ClientError::with_kind(
            ClientErrorKind::Wallet,
            "caller wallet address is required; configure a wallet, pass --caller, or set OCTRA_CALLER",
        )
    })?;
    let private_key = first_string(&[
        options.private_key.clone(),
        wallet.priv_field.clone(),
        wallet.priv_.clone(),
        wallet.private_key.clone(),
        wallet.private_key_b64.clone(),
        env::var("OCTRA_PRIVATE_KEY_B64").ok(),
    ])
    .ok_or_else(|| {
        ClientError::with_kind(
            ClientErrorKind::Wallet,
            "wallet private key is required; pass --wallet or OCTRA_PRIVATE_KEY_B64",
        )
    })?;
    let signing_key = signing_key_from_text(&private_key)?;
    let derived_pub = general_purpose::STANDARD.encode(signing_key.verifying_key().to_bytes());
    let public_key = first_string(&[
        options.public_key.clone(),
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
        private_key,
        public_key,
    })
}

fn first_string(values: &[Option<String>]) -> Option<String> {
    values
        .iter()
        .find_map(|value| value.as_ref().filter(|v| !v.is_empty()).cloned())
}

fn choose_session_rpc(
    explicit_rpc: Option<String>,
    target_rpc: Option<String>,
    config_rpc: Option<String>,
    wallet_rpc: Option<String>,
) -> Option<String> {
    first_string(&[explicit_rpc, target_rpc, config_rpc, wallet_rpc])
}

fn signing_key_from_text(text: &str) -> Result<SigningKey> {
    let cleaned = text.trim();
    let raw = general_purpose::STANDARD
        .decode(cleaned)
        .or_else(|_| hex::decode(cleaned))
        .map_err(|_| {
            ClientError::with_kind(ClientErrorKind::Wallet, "private key must be base64 or hex")
        })?;
    if raw.len() < 32 {
        return Err(ClientError::with_kind(
            ClientErrorKind::Wallet,
            "private key must decode to at least 32 bytes",
        ));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&raw[..32]);
    Ok(SigningKey::from_bytes(&seed))
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
}
