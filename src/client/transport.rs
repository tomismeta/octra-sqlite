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
const MAX_RPC_ATTEMPTS: usize = 4;

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
        let retryable = method != "octra_submit";
        for attempt in 1..=MAX_RPC_ATTEMPTS {
            let response = match self.client.post(rpc).json(&body).send() {
                Ok(response) => response,
                Err(error) => {
                    let message = format!("calling {method}: {error}");
                    self.trace_rpc(rpc, method, &body, None, None, Some(&message));
                    if should_retry_transport(attempt, retryable) {
                        sleep_before_retry(attempt, None);
                        continue;
                    }
                    return Err(ClientError::with_kind(ClientErrorKind::Transport, message));
                }
            };
            let status = response.status();
            let status_code = status.as_u16();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_retry_after);
            let text = match response.text() {
                Ok(text) => text,
                Err(error) => {
                    let message = format!("reading {method} response body: {error}");
                    self.trace_rpc(rpc, method, &body, None, Some(status_code), Some(&message));
                    if should_retry_http(attempt, retryable, status_code) {
                        sleep_before_retry(attempt, retry_after);
                        continue;
                    }
                    return Err(ClientError::with_kind(ClientErrorKind::Transport, message));
                }
            };
            let payload: Value = match serde_json::from_str(&text) {
                Ok(payload) => payload,
                Err(error) => {
                    let message = format!(
                        "decoding {method} non-JSON response from HTTP {status}: {error}; body: {}",
                        preview_text(&text, 512)
                    );
                    self.trace_rpc(rpc, method, &body, None, Some(status_code), Some(&message));
                    if should_retry_http(attempt, retryable, status_code)
                        || should_retry_non_json(attempt, retryable, &text)
                    {
                        sleep_before_retry(attempt, retry_after);
                        continue;
                    }
                    return Err(ClientError::with_kind(ClientErrorKind::Decode, message));
                }
            };
            if !status.is_success() {
                let message = format!(
                    "{method} failed with HTTP {status}: {}",
                    preview_value(&payload, 1024)
                );
                self.trace_rpc(
                    rpc,
                    method,
                    &body,
                    Some(&payload),
                    Some(status_code),
                    Some(&message),
                );
                if should_retry_http(attempt, retryable, status_code) {
                    sleep_before_retry(attempt, retry_after);
                    continue;
                }
                return Err(ClientError::with_kind(ClientErrorKind::Transport, message));
            }
            if let Some(error) = payload.get("error") {
                let message = format!("{method} failed: {}", preview_value(error, 1024));
                self.trace_rpc(
                    rpc,
                    method,
                    &body,
                    Some(&payload),
                    Some(status_code),
                    Some(&message),
                );
                if should_retry_rpc_error(attempt, retryable, error) {
                    sleep_before_retry(attempt, retry_after);
                    continue;
                }
                return Err(ClientError::with_kind(ClientErrorKind::Rpc, message));
            }
            self.trace_rpc(rpc, method, &body, Some(&payload), Some(status_code), None);
            return Ok(payload.get("result").cloned().unwrap_or(Value::Null));
        }
        Err(ClientError::with_kind(
            ClientErrorKind::Transport,
            format!("{method} failed after retry attempts"),
        ))
    }
}

#[cfg(feature = "http")]
fn should_retry_transport(attempt: usize, retryable: bool) -> bool {
    retryable && attempt < MAX_RPC_ATTEMPTS
}

#[cfg(feature = "http")]
fn should_retry_http(attempt: usize, retryable: bool, status_code: u16) -> bool {
    should_retry_transport(attempt, retryable)
        && matches!(status_code, 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

#[cfg(feature = "http")]
fn should_retry_non_json(attempt: usize, retryable: bool, text: &str) -> bool {
    let first = text.trim_start().chars().next();
    should_retry_transport(attempt, retryable) && matches!(first, Some('<' | '\u{feff}'))
}

#[cfg(feature = "http")]
fn should_retry_rpc_error(attempt: usize, retryable: bool, error: &Value) -> bool {
    if !should_retry_transport(attempt, retryable) {
        return false;
    }
    let code = error.get("code").and_then(Value::as_i64);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(code, Some(429) | Some(-32029))
        || message.contains("too many requests")
        || message.contains("rate limit")
        || message.contains("temporarily unavailable")
}

#[cfg(feature = "http")]
fn parse_retry_after(value: &str) -> Option<Duration> {
    let seconds = value.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds.min(30)))
}

#[cfg(feature = "http")]
fn sleep_before_retry(attempt: usize, retry_after: Option<Duration>) {
    std::thread::sleep(retry_after.unwrap_or_else(|| {
        let millis = 500_u64.saturating_mul(2_u64.saturating_pow((attempt - 1) as u32));
        Duration::from_millis(millis.min(5_000))
    }));
}

#[cfg(feature = "http")]
fn preview_value(value: &Value, limit: usize) -> String {
    preview_text(
        &serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
        limit,
    )
}

#[cfg(feature = "http")]
fn preview_text(text: &str, limit: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() <= limit {
        compact
    } else {
        format!(
            "{}...<truncated {} bytes>",
            truncate_to_char_boundary(&compact, limit),
            compact.len()
        )
    }
}

#[cfg(feature = "http")]
fn truncate_to_char_boundary(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = 0usize;
    for (index, _) in text.char_indices() {
        if index > limit {
            break;
        }
        end = index;
    }
    &text[..end]
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

    #[test]
    fn retry_policy_is_mainnet_safe() {
        assert!(should_retry_http(1, true, 429));
        assert!(should_retry_http(1, true, 503));
        assert!(!should_retry_http(MAX_RPC_ATTEMPTS, true, 429));
        assert!(!should_retry_http(1, false, 429));
        assert!(should_retry_rpc_error(
            1,
            true,
            &json!({"code":429,"message":"Too Many Requests"})
        ));
        assert!(should_retry_rpc_error(
            1,
            true,
            &json!({"message":"temporarily unavailable"})
        ));
        assert!(!should_retry_rpc_error(
            1,
            true,
            &json!({"code":-32000,"message":"wasm export returned 1"})
        ));
    }

    #[test]
    fn response_previews_are_compact_and_utf8_safe() {
        let text = "αβγδε ζηθικ λμνξο";
        let preview = preview_text(text, 7);
        assert!(preview.contains("<truncated"));
        assert!(preview.is_char_boundary(preview.len()));
        assert!(preview_text("<html> too many requests </html>", 512).contains("<html>"));
        assert!(should_retry_non_json(1, true, "  <html>429</html>"));
    }
}
