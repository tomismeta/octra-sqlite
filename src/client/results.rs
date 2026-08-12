use super::error::{Error, ErrorKind, Result};
use serde_json::Value;

/// Result of read SQL.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    /// Column names returned by SQLite.
    pub columns: Vec<String>,
    /// Rows as JSON values in column order.
    pub rows: Vec<Vec<Value>>,
    /// Number of returned rows.
    pub row_count: usize,
    raw: Value,
}

impl QueryResult {
    /// Decode a raw Circle query response into validated typed rows.
    pub fn from_value(value: Value) -> Result<Self> {
        let columns = value
            .get("columns")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::with_kind(ErrorKind::Decode, "query result missing columns"))?
            .iter()
            .map(|column| {
                column.as_str().map(str::to_string).ok_or_else(|| {
                    Error::with_kind(ErrorKind::Decode, "query result column must be a string")
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let rows = value
            .get("rows")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::with_kind(ErrorKind::Decode, "query result missing rows"))?
            .iter()
            .map(|row| {
                row.as_array().cloned().ok_or_else(|| {
                    Error::with_kind(ErrorKind::Decode, "query result row must be an array")
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let row_count = match value.get("row_count").and_then(Value::as_u64) {
            Some(count) => usize::try_from(count).map_err(|_| {
                Error::with_kind(ErrorKind::Decode, "query result row_count exceeds usize")
            })?,
            None => rows.len(),
        };
        if row_count != rows.len() {
            return Err(Error::with_kind(
                ErrorKind::Decode,
                format!(
                    "query result row_count {row_count} does not match {} rows",
                    rows.len()
                ),
            ));
        }
        for row in &rows {
            if row.len() != columns.len() {
                return Err(Error::with_kind(
                    ErrorKind::Decode,
                    format!(
                        "query result row has {} cells but {} columns",
                        row.len(),
                        columns.len()
                    ),
                ));
            }
        }
        Ok(Self {
            columns,
            rows,
            row_count,
            raw: value,
        })
    }

    /// Return the original query response.
    pub fn raw(&self) -> &Value {
        &self.raw
    }
}

/// Submitted Octra transaction returned by no-wait write paths.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmittedTransaction {
    /// Target Circle ID when known.
    pub circle: Option<String>,
    /// Submitting wallet address when known.
    pub wallet: Option<String>,
    /// Transaction hash when the RPC returned one.
    pub tx_hash: Option<String>,
    /// Raw submit result.
    pub result: Value,
}

impl SubmittedTransaction {
    /// Decode a raw transaction-submission response.
    pub fn from_value(value: Value) -> Result<Self> {
        Ok(Self {
            circle: string_field(&value, "circle"),
            wallet: string_field(&value, "wallet"),
            tx_hash: string_field(&value, "tx_hash"),
            result: value.get("result").cloned().ok_or_else(|| {
                Error::with_kind(ErrorKind::Rpc, "submitted transaction missing result")
            })?,
        })
    }
}

/// Result of a write that has been submitted and confirmed.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecuteResult {
    /// Submitted transaction metadata.
    pub submitted: SubmittedTransaction,
    /// Confirmed transaction receipt.
    pub receipt: Value,
}

impl ExecuteResult {
    /// Decode a confirmed execution response and fail if its receipt failed.
    pub fn from_value(value: Value) -> Result<Self> {
        let submitted = SubmittedTransaction::from_value(value.clone())?;
        let receipt = value
            .get("receipt")
            .cloned()
            .ok_or_else(|| Error::with_kind(ErrorKind::Receipt, "exec result missing receipt"))?;
        ensure_receipt_success(&receipt)?;
        Ok(Self { submitted, receipt })
    }

    /// Return the deterministic runtime effort reported by Octra, when present.
    pub fn effort(&self) -> Option<u64> {
        u64_field(&self.receipt, "effort")
    }
}

/// Circle-backed SQLite storage state and effective limits.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageInfo {
    /// Storage implementation used by the Circle program.
    pub storage: String,
    /// Stable-storage key containing the active database metadata.
    pub meta_key: String,
    /// Whether database metadata has been initialized.
    pub exists: bool,
    /// SQLite page size in bytes.
    pub page_size: u64,
    /// Current SQLite file size in bytes.
    pub file_bytes: u64,
    /// Current SQLite page count.
    pub page_count: u64,
    /// Active storage generation.
    pub generation: u64,
    /// Atomic commit protocol used by the page VFS.
    pub commit_protocol: String,
    /// Stored metadata format version.
    pub meta_version: u64,
    /// Last accepted owner-write sequence, when used by the deployed engine.
    pub owner_sequence: Option<u64>,
    /// Maximum pages that one execution may dirty.
    pub max_dirty_pages: u64,
    /// Maximum SQLite pages allowed by this engine.
    pub max_db_pages: u64,
    /// Maximum SQLite file size in bytes, when reported by the engine.
    pub max_db_file_bytes: Option<u64>,
    /// Circle stable-storage ceiling in bytes, when reported by the engine.
    pub stable_storage_limit_bytes: Option<u64>,
    /// Deterministic read-query SQLite step budget, when reported by the engine.
    pub query_vdbe_steps: Option<u64>,
    /// Deterministic write-execution SQLite step budget, when reported by the engine.
    pub exec_vdbe_steps: Option<u64>,
    raw: Value,
}

impl StorageInfo {
    /// Decode a raw `storage_info` response.
    pub fn from_value(value: Value) -> Result<Self> {
        Ok(Self {
            storage: required_string_field(&value, "storage")?,
            meta_key: required_string_field(&value, "meta_key")?,
            exists: value
                .get("exists")
                .and_then(Value::as_bool)
                .ok_or_else(|| decode_field_error("storage_info", "exists"))?,
            page_size: required_u64_field(&value, "page_size")?,
            file_bytes: required_u64_field(&value, "file_bytes")?,
            page_count: required_u64_field(&value, "page_count")?,
            generation: required_u64_field(&value, "generation")?,
            commit_protocol: required_string_field(&value, "commit_protocol")?,
            meta_version: required_u64_field(&value, "meta_version")?,
            owner_sequence: optional_u64_field(&value, "owner_sequence")?,
            max_dirty_pages: required_u64_field(&value, "max_dirty_pages")?,
            max_db_pages: required_u64_field(&value, "max_db_pages")?,
            max_db_file_bytes: optional_u64_field(&value, "max_db_file_bytes")?,
            stable_storage_limit_bytes: optional_u64_field(&value, "stable_storage_limit_bytes")?,
            query_vdbe_steps: optional_u64_field(&value, "query_vdbe_steps")?,
            exec_vdbe_steps: optional_u64_field(&value, "exec_vdbe_steps")?,
            raw: value,
        })
    }

    /// Return the original `storage_info` response.
    pub fn raw(&self) -> &Value {
        &self.raw
    }
}

/// Deployed Circle program metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramInfo {
    /// Circle program version when reported by Octra.
    pub version: Option<String>,
    /// Deployed personalized WASM SHA-256 when reported by Octra.
    pub code_hash: Option<String>,
    /// Deployed WASM byte length when reported by Octra.
    pub code_bytes: Option<u64>,
    raw: Value,
}

impl ProgramInfo {
    /// Decode a raw Circle program-info response.
    pub fn from_value(value: Value) -> Result<Self> {
        Ok(Self {
            version: string_field(&value, "version"),
            code_hash: string_field(&value, "code_hash"),
            code_bytes: value
                .get("code_bytes")
                .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok())),
            raw: value,
        })
    }

    /// Return the original program-info response.
    pub fn raw(&self) -> &Value {
        &self.raw
    }
}

/// Owner-write authorization metadata exposed by the Circle program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthInfo {
    /// Whether owner-write authorization is configured.
    pub configured: bool,
    /// Database identity bound into OSW1 owner-write intents.
    pub db_id: String,
    /// Owner public key accepted by the Circle program.
    pub owner_pubkey: Option<String>,
    /// Next owner-write sequence when the Circle reports it.
    pub owner_sequence: Option<u64>,
}

pub(super) fn ensure_receipt_success(receipt: &Value) -> Result<()> {
    let sql_error = event_values(receipt, "octra.sqlite.error");
    let failed = receipt.get("success").and_then(Value::as_bool) != Some(true)
        || receipt.get("error").is_some_and(|error| !error.is_null())
        || sql_error.is_some();
    if failed {
        let detail = sql_error
            .as_deref()
            .map(format_sql_error_event)
            .unwrap_or_else(|| receipt_error_text(receipt));
        let message = format!("SQL execution failed: {detail}");
        return match sql_error.as_deref().and_then(sql_error_code) {
            Some(code) => Err(Error::with_code(ErrorKind::Receipt, code, message)),
            None => Err(Error::with_kind(ErrorKind::Receipt, message)),
        };
    }
    Ok(())
}

fn event_values(receipt: &Value, topic: &str) -> Option<String> {
    receipt
        .get("events")?
        .as_array()?
        .iter()
        .find(|event| event.get("event").and_then(Value::as_str) == Some(topic))
        .and_then(|event| event.get("values"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(value_to_event_text)
                .collect::<Vec<_>>()
                .join(", ")
        })
}

fn receipt_error_text(receipt: &Value) -> String {
    receipt
        .get("error")
        .filter(|error| !error.is_null())
        .map(value_to_compact_text)
        .unwrap_or_else(|| value_to_compact_text(receipt))
}

fn value_to_compact_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn value_to_event_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value_to_compact_text(value))
}

fn format_sql_error_event(error: &str) -> String {
    match error.split_once(':') {
        Some((code, detail)) if !detail.is_empty() => {
            format!("database error ({code}): {detail}")
        }
        _ => error.to_string(),
    }
}

fn sql_error_code(error: &str) -> Option<&str> {
    let code = error.split_once(':').map(|(code, _)| code).unwrap_or(error);
    (!code.is_empty()).then_some(code)
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn u64_field(value: &Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
}

fn required_u64_field(value: &Value, key: &str) -> Result<u64> {
    u64_field(value, key).ok_or_else(|| decode_field_error("storage_info", key))
}

fn optional_u64_field(value: &Value, key: &str) -> Result<Option<u64>> {
    match value.get(key) {
        None => Ok(None),
        Some(_) => u64_field(value, key)
            .map(Some)
            .ok_or_else(|| decode_field_error("storage_info", key)),
    }
}

fn required_string_field(value: &Value, key: &str) -> Result<String> {
    string_field(value, key).ok_or_else(|| decode_field_error("storage_info", key))
}

fn decode_field_error(subject: &str, key: &str) -> Error {
    Error::with_kind(
        ErrorKind::Decode,
        format!("{subject} missing or invalid {key}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn query_result_validates_rectangular_rows() {
        let error = QueryResult::from_value(json!({
            "columns": ["a", "b"],
            "rows": [[1]],
            "row_count": 1,
        }))
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode);
    }

    #[test]
    fn receipt_success_with_sql_error_event_is_failed_execution() {
        let receipt = json!({
            "success": true,
            "error": null,
            "events": [{
                "event": "octra.sqlite.error",
                "values": ["sqlite_exec_failed:no such table: correction"]
            }]
        });
        let error = ensure_receipt_success(&receipt).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Receipt);
        assert!(
            error
                .to_string()
                .contains("database error (sqlite_exec_failed): no such table: correction")
        );
    }

    #[test]
    fn receipt_without_explicit_success_fails_closed() {
        let error = ensure_receipt_success(&json!({"events": []})).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Receipt);
    }

    #[test]
    fn execute_result_exposes_numeric_or_string_effort() {
        let result = |effort| {
            ExecuteResult::from_value(json!({
                "result": {"tx_hash": "abc"},
                "receipt": {"success": true, "effort": effort},
            }))
            .unwrap()
        };
        assert_eq!(result(json!(227)).effort(), Some(227));
        assert_eq!(result(json!("17117")).effort(), Some(17_117));
    }

    #[test]
    fn storage_info_accepts_historical_shape_without_new_limits() {
        let info = StorageInfo::from_value(json!({
            "storage": "circle_key_value_page_vfs",
            "meta_key": "octra.sqlite.vfs.v1.meta",
            "exists": false,
            "page_size": 4096,
            "file_bytes": 0,
            "page_count": 0,
            "generation": 0,
            "commit_protocol": "generation_manifest_v4",
            "meta_version": 4,
            "max_dirty_pages": 1024,
            "max_db_pages": 8192,
        }))
        .unwrap();
        assert_eq!(info.max_db_file_bytes, None);
        assert_eq!(info.stable_storage_limit_bytes, None);
        assert_eq!(info.query_vdbe_steps, None);
        assert_eq!(info.exec_vdbe_steps, None);
        assert_eq!(info.owner_sequence, None);
    }

    #[test]
    fn storage_info_rejects_missing_stable_fields() {
        let error = StorageInfo::from_value(json!({})).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.to_string().contains("storage"));
    }

    #[test]
    fn storage_info_rejects_malformed_optional_fields() {
        let error = StorageInfo::from_value(json!({
            "storage": "circle_key_value_page_vfs",
            "meta_key": "octra.sqlite.vfs.v1.meta",
            "exists": false,
            "page_size": 4096,
            "file_bytes": 0,
            "page_count": 0,
            "generation": 0,
            "commit_protocol": "generation_manifest_v4",
            "meta_version": 4,
            "owner_sequence": 0,
            "max_dirty_pages": 1024,
            "max_db_pages": 8192,
            "max_db_file_bytes": "invalid",
        }))
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.to_string().contains("max_db_file_bytes"));
    }
}
