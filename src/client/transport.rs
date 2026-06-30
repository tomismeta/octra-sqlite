use super::error::Result;
#[cfg(feature = "http")]
use super::error::{ClientError, ClientErrorKind};
#[cfg(feature = "http")]
use serde_json::json;
use serde_json::Value;
#[cfg(feature = "http")]
use sha2::{Digest, Sha256};
#[cfg(feature = "http")]
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub trait Transport {
    fn call(&self, rpc: &str, method: &str, params: Value) -> Result<Value>;
}

#[cfg(feature = "http")]
#[derive(Clone)]
pub struct HttpTransport {
    client: reqwest::blocking::Client,
    trace: Option<Arc<Mutex<RpcTraceWriter>>>,
}

#[cfg(feature = "http")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RpcTraceMode {
    #[default]
    Full,
    Summary,
    RequestOnly,
    ResponseMeta,
}

#[cfg(feature = "http")]
struct RpcTraceWriter {
    file: std::fs::File,
    sequence: u64,
    mode: RpcTraceMode,
}

#[cfg(feature = "http")]
impl Default for HttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "http")]
impl HttpTransport {
    pub fn new() -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self {
            client,
            trace: None,
        }
    }

    pub fn with_trace_jsonl(path: &Path) -> Result<Self> {
        Self::with_trace_jsonl_mode(path, RpcTraceMode::Full)
    }

    pub fn with_trace_jsonl_mode(path: &Path, mode: RpcTraceMode) -> Result<Self> {
        let mut transport = Self::new();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                ClientError::with_kind(
                    ClientErrorKind::Io,
                    format!("creating RPC trace directory {}: {error}", parent.display()),
                )
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .map_err(|error| {
                ClientError::with_kind(
                    ClientErrorKind::Io,
                    format!("opening RPC trace {}: {error}", path.display()),
                )
            })?;
        transport.trace = Some(Arc::new(Mutex::new(RpcTraceWriter {
            file,
            sequence: 0,
            mode,
        })));
        Ok(transport)
    }

    fn trace_rpc(
        &self,
        rpc: &str,
        method: &str,
        request: &Value,
        response: Option<&Value>,
        http_status: Option<u16>,
        error: Option<&str>,
    ) {
        let Some(trace) = &self.trace else {
            return;
        };
        let Ok(mut trace) = trace.lock() else {
            return;
        };
        trace.sequence += 1;
        let mut event = json!({
            "schema": "octra-sqlite.rpc-trace.v1",
            "mode": trace_mode_name(trace.mode),
            "sequence": trace.sequence,
            "timestamp_ms": unix_timestamp_ms(),
            "rpc": rpc,
            "method": method,
            "http_status": http_status,
            "ok": error.is_none(),
            "error": error,
        });
        match trace.mode {
            RpcTraceMode::Full => {
                event["request"] = request.clone();
                event["response"] = response.cloned().unwrap_or(Value::Null);
                event["request_meta"] = trace_value_meta(request);
                event["response_meta"] = trace_optional_value_meta(response);
            }
            RpcTraceMode::Summary => {
                let request_meta = trace_value_meta(request);
                let response_meta = trace_optional_value_meta(response);
                event["request_bytes"] = request_meta["bytes"].clone();
                event["request_sha256"] = request_meta["sha256"].clone();
                event["response_bytes"] = response_meta["bytes"].clone();
                event["response_sha256"] = response_meta["sha256"].clone();
            }
            RpcTraceMode::RequestOnly => {
                event["request"] = request.clone();
                event["response_meta"] = trace_optional_value_meta(response);
            }
            RpcTraceMode::ResponseMeta => {
                event["request_meta"] = trace_value_meta(request);
                event["response_meta"] = trace_optional_value_meta(response);
            }
        }
        if serde_json::to_writer(&mut trace.file, &event).is_err() {
            return;
        }
        let _ = trace.file.write_all(b"\n");
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
        let response = match self.client.post(rpc).json(&body).send() {
            Ok(response) => response,
            Err(error) => {
                let message = format!("calling {method}: {error}");
                self.trace_rpc(rpc, method, &body, None, None, Some(&message));
                return Err(ClientError::with_kind(ClientErrorKind::Transport, message));
            }
        };
        let status = response.status();
        let status_code = status.as_u16();
        let payload: Value = match response.json() {
            Ok(payload) => payload,
            Err(error) => {
                let message = format!("decoding {method} response: {error}");
                self.trace_rpc(rpc, method, &body, None, Some(status_code), Some(&message));
                return Err(ClientError::with_kind(ClientErrorKind::Decode, message));
            }
        };
        if !status.is_success() {
            let message = format!("{method} failed with HTTP {status}: {payload}");
            self.trace_rpc(
                rpc,
                method,
                &body,
                Some(&payload),
                Some(status_code),
                Some(&message),
            );
            return Err(ClientError::with_kind(ClientErrorKind::Transport, message));
        }
        if let Some(error) = payload.get("error") {
            let message = format!("{method} failed: {error}");
            self.trace_rpc(
                rpc,
                method,
                &body,
                Some(&payload),
                Some(status_code),
                Some(&message),
            );
            return Err(ClientError::with_kind(ClientErrorKind::Rpc, message));
        }
        self.trace_rpc(rpc, method, &body, Some(&payload), Some(status_code), None);
        Ok(payload.get("result").cloned().unwrap_or(Value::Null))
    }
}

#[cfg(feature = "http")]
fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(feature = "http")]
fn trace_mode_name(mode: RpcTraceMode) -> &'static str {
    match mode {
        RpcTraceMode::Full => "full",
        RpcTraceMode::Summary => "summary",
        RpcTraceMode::RequestOnly => "request_only",
        RpcTraceMode::ResponseMeta => "response_meta",
    }
}

#[cfg(feature = "http")]
fn trace_optional_value_meta(value: Option<&Value>) -> Value {
    match value {
        Some(value) => trace_value_meta(value),
        None => json!({
            "bytes": null,
            "sha256": null,
        }),
    }
}

#[cfg(feature = "http")]
fn trace_value_meta(value: &Value) -> Value {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    json!({
        "bytes": bytes.len(),
        "sha256": hex::encode(Sha256::digest(&bytes)),
    })
}

#[cfg(all(test, feature = "http"))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trace_writer_records_json_rpc_request_and_response() {
        let path = std::env::temp_dir().join(format!(
            "octra-sqlite-rpc-trace-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let transport = HttpTransport::with_trace_jsonl(&path).unwrap();
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "octra_circleViewAuth",
            "params": ["octCircle"]
        });
        let response = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "ok"
        });

        transport.trace_rpc(
            "https://devnet.octrascan.io/rpc",
            "octra_circleViewAuth",
            &request,
            Some(&response),
            Some(200),
            None,
        );

        let text = std::fs::read_to_string(&path).unwrap();
        let event: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(event["schema"], "octra-sqlite.rpc-trace.v1");
        assert_eq!(event["mode"], "full");
        assert_eq!(event["sequence"], 1);
        assert_eq!(event["method"], "octra_circleViewAuth");
        assert_eq!(event["request"], request);
        assert_eq!(event["response"], response);
        assert_eq!(
            event["request_meta"]["bytes"],
            serde_json::to_vec(&request).unwrap().len()
        );
        assert_eq!(event["request_meta"]["sha256"].as_str().unwrap().len(), 64);
        assert!(event["error"].is_null());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn trace_writer_summary_omits_rpc_bodies() {
        let path = std::env::temp_dir().join(format!(
            "octra-sqlite-rpc-trace-summary-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let transport = HttpTransport::with_trace_jsonl_mode(&path, RpcTraceMode::Summary).unwrap();
        transport.trace_rpc(
            "https://devnet.octrascan.io/rpc",
            "octra_circleViewAuth",
            &json!({"params": ["select * from artist;"]}),
            Some(&json!({"result": {"rows": [[1]]}})),
            Some(200),
            None,
        );

        let text = std::fs::read_to_string(&path).unwrap();
        let event: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(event["mode"], "summary");
        assert!(event.get("request").is_none());
        assert!(event.get("response").is_none());
        assert!(event["request_bytes"].as_u64().unwrap() > 0);
        assert_eq!(event["request_sha256"].as_str().unwrap().len(), 64);
        assert!(event["response_bytes"].as_u64().unwrap() > 0);
        assert_eq!(event["response_sha256"].as_str().unwrap().len(), 64);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn trace_writer_is_best_effort_after_open() {
        let path = std::env::temp_dir().join(format!(
            "octra-sqlite-rpc-trace-poison-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let transport = HttpTransport::with_trace_jsonl(&path).unwrap();
        let trace = transport.trace.as_ref().unwrap().clone();
        let _ = std::thread::spawn(move || {
            let _guard = trace.lock().unwrap();
            panic!("poison trace lock");
        })
        .join();

        transport.trace_rpc(
            "https://devnet.octrascan.io/rpc",
            "octra_circleViewAuth",
            &json!({"params": []}),
            None,
            None,
            Some("real rpc error"),
        );

        let _ = std::fs::remove_file(path);
    }
}
