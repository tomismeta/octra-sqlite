use super::error::Result;
#[cfg(feature = "http")]
use super::error::{ClientError, ClientErrorKind};
#[cfg(feature = "http")]
use serde_json::json;
use serde_json::Value;
#[cfg(feature = "http")]
use std::time::Duration;

pub trait Transport {
    fn call(&self, rpc: &str, method: &str, params: Value) -> Result<Value>;
}

#[cfg(feature = "http")]
#[derive(Clone)]
pub struct HttpTransport {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "http")]
impl Default for HttpTransport {
    fn default() -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client }
    }
}

#[cfg(feature = "http")]
impl Transport for HttpTransport {
    fn call(&self, rpc: &str, method: &str, params: Value) -> Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        let response = self.client.post(rpc).json(&body).send().map_err(|error| {
            ClientError::with_kind(
                ClientErrorKind::Transport,
                format!("calling {method}: {error}"),
            )
        })?;
        let status = response.status();
        let payload: Value = response.json().map_err(|error| {
            ClientError::with_kind(
                ClientErrorKind::Decode,
                format!("decoding {method} response: {error}"),
            )
        })?;
        if !status.is_success() {
            return Err(ClientError::with_kind(
                ClientErrorKind::Transport,
                format!("{method} failed with HTTP {status}: {payload}"),
            ));
        }
        if let Some(error) = payload.get("error") {
            return Err(ClientError::with_kind(
                ClientErrorKind::Rpc,
                format!("{method} failed: {error}"),
            ));
        }
        Ok(payload.get("result").cloned().unwrap_or(Value::Null))
    }
}
