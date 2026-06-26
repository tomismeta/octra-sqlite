use super::error::{ClientError, ClientErrorKind, Result};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub row_count: usize,
    raw: Value,
}

impl QueryResult {
    pub fn from_value(value: Value) -> Result<Self> {
        let columns = value
            .get("columns")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ClientError::with_kind(ClientErrorKind::Decode, "query result missing columns")
            })?
            .iter()
            .map(|column| {
                column.as_str().map(str::to_string).ok_or_else(|| {
                    ClientError::with_kind(
                        ClientErrorKind::Decode,
                        "query result column must be a string",
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let rows = value
            .get("rows")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ClientError::with_kind(ClientErrorKind::Decode, "query result missing rows")
            })?
            .iter()
            .map(|row| {
                row.as_array().cloned().ok_or_else(|| {
                    ClientError::with_kind(
                        ClientErrorKind::Decode,
                        "query result row must be an array",
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let row_count = value
            .get("row_count")
            .and_then(Value::as_u64)
            .map(|count| count as usize)
            .unwrap_or(rows.len());
        if row_count != rows.len() {
            return Err(ClientError::with_kind(
                ClientErrorKind::Decode,
                format!(
                    "query result row_count {row_count} does not match {} rows",
                    rows.len()
                ),
            ));
        }
        for row in &rows {
            if row.len() != columns.len() {
                return Err(ClientError::with_kind(
                    ClientErrorKind::Decode,
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

    pub fn raw(&self) -> &Value {
        &self.raw
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubmittedTx {
    pub circle: Option<String>,
    pub wallet: Option<String>,
    pub tx_hash: Option<String>,
    pub result: Value,
}

impl SubmittedTx {
    pub fn from_value(value: Value) -> Result<Self> {
        Ok(Self {
            circle: string_field(&value, "circle"),
            wallet: string_field(&value, "wallet"),
            tx_hash: string_field(&value, "tx_hash"),
            result: value.get("result").cloned().ok_or_else(|| {
                ClientError::with_kind(ClientErrorKind::Rpc, "submitted transaction missing result")
            })?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecResult {
    pub submitted: SubmittedTx,
    pub receipt: Value,
}

impl ExecResult {
    pub fn from_value(value: Value) -> Result<Self> {
        let submitted = SubmittedTx::from_value(value.clone())?;
        let receipt = value.get("receipt").cloned().ok_or_else(|| {
            ClientError::with_kind(ClientErrorKind::Receipt, "exec result missing receipt")
        })?;
        ensure_receipt_success(&receipt)?;
        Ok(Self { submitted, receipt })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgramInfo {
    pub version: Option<String>,
    pub code_hash: Option<String>,
    pub code_bytes: Option<u64>,
    raw: Value,
}

impl ProgramInfo {
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

    pub fn raw(&self) -> &Value {
        &self.raw
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthInfo {
    pub configured: bool,
    pub db_id: String,
    pub owner_pubkey: Option<String>,
    pub owner_sequence: Option<u64>,
}

pub(super) fn ensure_receipt_success(receipt: &Value) -> Result<()> {
    let sql_error = event_values(receipt, "octra.sqlite.error");
    let failed = receipt.get("success").and_then(Value::as_bool) == Some(false)
        || receipt.get("error").is_some_and(|error| !error.is_null())
        || sql_error.is_some();
    if failed {
        return Err(ClientError::with_kind(
            ClientErrorKind::Receipt,
            format!(
                "SQL execution failed: {}",
                sql_error
                    .map(|error| format_sql_error_event(&error))
                    .unwrap_or_else(|| receipt_error_text(receipt))
            ),
        ));
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

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
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
        assert_eq!(error.kind(), ClientErrorKind::Decode);
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
        assert_eq!(error.kind(), ClientErrorKind::Receipt);
        assert!(error
            .to_string()
            .contains("database error (sqlite_exec_failed): no such table: correction"));
    }
}
