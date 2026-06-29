use super::error::Result;
#[cfg(feature = "http")]
use super::error::{ClientError, ClientErrorKind};
#[cfg(feature = "http")]
use serde_json::json;
use serde_json::Value;
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
struct RpcTraceWriter {
    file: std::fs::File,
    sequence: u64,
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
        transport.trace = Some(Arc::new(Mutex::new(RpcTraceWriter { file, sequence: 0 })));
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
        let event = json!({
            "schema": "octra-sqlite.rpc-trace.v1",
            "sequence": trace.sequence,
            "timestamp_ms": unix_timestamp_ms(),
            "rpc": rpc,
            "method": method,
            "http_status": http_status,
            "request": request,
            "response": response,
            "error": error,
        });
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
        assert_eq!(event["sequence"], 1);
        assert_eq!(event["method"], "octra_circleViewAuth");
        assert_eq!(event["request"], request);
        assert_eq!(event["response"], response);
        assert!(event["error"].is_null());
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
