use anyhow::{Context, Result};
use serde_json::Value;
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::io::{self, IsTerminal};
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum OutputMode {
    Table,
    List,
    Json,
    Line,
    Csv,
}

impl OutputMode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            OutputMode::Table => "table",
            OutputMode::List => "list",
            OutputMode::Json => "json",
            OutputMode::Line => "line",
            OutputMode::Csv => "csv",
        }
    }
}

pub(crate) fn print_exec_result(result: &Value) -> Result<()> {
    print!("{}", format_exec_result(result)?);
    Ok(())
}

pub(crate) fn dim(text: impl AsRef<str>) -> String {
    style("2", text.as_ref())
}

pub(crate) fn strong(text: impl AsRef<str>) -> String {
    style("1", text.as_ref())
}

pub(crate) fn format_field(label: &str, detail: impl AsRef<str>) -> String {
    format_field_with_width(label, detail.as_ref(), terminal_width())
}

pub(crate) fn format_status_line(status: &str, label: &str, detail: impl AsRef<str>) -> String {
    format_status_line_with_width(status, label, detail.as_ref(), terminal_width())
}

pub(crate) fn hyperlink(label: impl AsRef<str>, url: impl AsRef<str>) -> String {
    let label = label.as_ref();
    let url = url.as_ref();
    if terminal_style_enabled() {
        format!("\x1b]8;;{url}\x1b\\{label}\x1b]8;;\x1b\\")
    } else {
        label.to_string()
    }
}

pub(crate) fn terminal_style_enabled() -> bool {
    io::stdout().is_terminal()
        && env::var_os("NO_COLOR").is_none()
        && env::var_os("OCTRA_SQLITE_PLAIN").is_none()
        && env::var("TERM").map(|term| term != "dumb").unwrap_or(true)
}

fn style(code: &str, text: &str) -> String {
    if terminal_style_enabled() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn terminal_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|columns| columns.parse::<usize>().ok())
        .filter(|columns| *columns >= 40)
        .unwrap_or(100)
}

fn format_field_with_width(label: &str, detail: &str, width: usize) -> String {
    format_labeled_line(None, label, detail, width)
}

fn format_status_line_with_width(status: &str, label: &str, detail: &str, width: usize) -> String {
    format_labeled_line(Some(status), label, detail, width)
}

fn format_labeled_line(status: Option<&str>, label: &str, detail: &str, width: usize) -> String {
    let styled_label = dim(format!("{label}:"));
    let first = match status {
        Some(status) => format!("{} {styled_label}", strong(status)),
        None => styled_label,
    };
    let first_visible = status.map_or(0, |status| status.len() + 1) + label.len() + 2;
    let detail_visible = visible_len(detail);

    if !detail.contains('\n') && first_visible + 1 + detail_visible <= width {
        return format!("{first} {detail}\n");
    }

    let mut out = format!("{first}\n");
    for line in detail.lines() {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    if detail.is_empty() {
        out.push_str("  \n");
    }
    out
}

fn visible_len(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut len = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b {
            if index + 1 < bytes.len() && bytes[index + 1] == b'[' {
                index += 2;
                while index < bytes.len() {
                    let byte = bytes[index];
                    index += 1;
                    if (b'@'..=b'~').contains(&byte) {
                        break;
                    }
                }
                continue;
            }
            if index + 1 < bytes.len() && bytes[index + 1] == b']' {
                index += 2;
                while index < bytes.len() {
                    if bytes[index] == 0x07 {
                        index += 1;
                        break;
                    }
                    if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\'
                    {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
                continue;
            }
        }
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        len += 1;
        index += ch.len_utf8();
    }
    len
}

pub(crate) fn format_exec_result(result: &Value) -> Result<String> {
    let mut out = String::new();
    let receipt = result.get("receipt");
    let success = receipt
        .and_then(|receipt| receipt.get("success"))
        .and_then(Value::as_bool);
    let submitted_status = result
        .pointer("/result/status")
        .and_then(Value::as_str)
        .unwrap_or("submitted");

    let write_status = match success {
        Some(true) => "confirmed",
        Some(false) => "rejected",
        None => submitted_status,
    };
    out.push_str(&format_field("write", write_status));
    if let Some(circle) = result.get("circle").and_then(Value::as_str).or_else(|| {
        receipt
            .and_then(|receipt| receipt.get("contract"))
            .and_then(Value::as_str)
    }) {
        let circle = match result.get("circle_url").and_then(Value::as_str) {
            Some(url) => hyperlink(circle, url),
            None => circle.to_string(),
        };
        out.push_str(&format_field("circle", circle));
    }
    if let Some(url) = result.get("circle_url").and_then(Value::as_str) {
        out.push_str(&format_field("circle_url", url));
    }
    if let Some(wallet) = result.get("wallet").and_then(Value::as_str) {
        out.push_str(&format_field("wallet", wallet));
    }
    if let Some(hash) = result.get("tx_hash").and_then(Value::as_str) {
        let hash = match result.get("tx_url").and_then(Value::as_str) {
            Some(url) => hyperlink(hash, url),
            None => hash.to_string(),
        };
        out.push_str(&format_field("tx", hash));
    }
    if let Some(url) = result.get("tx_url").and_then(Value::as_str) {
        out.push_str(&format_field("tx_url", url));
    }
    if let Some(receipt) = receipt {
        out.push_str(&format_field(
            "receipt",
            receipt
                .get("success")
                .map(value_to_string)
                .unwrap_or_else(|| "unknown".to_string()),
        ));
        if let Some(error) = receipt.get("error").filter(|v| !v.is_null()) {
            out.push_str(&format_field("error", value_to_string(error)));
        }
        if let Some(auth) = auth_event(receipt) {
            out.push_str(&format_field("auth", auth));
        }
        if let Some(sql_error) = event_values(receipt, "octra.sqlite.error") {
            out.push_str(&format_field("sql_error", sql_error));
        }
        if let Some(sql) = event_values(receipt, "octra.sqlite.exec") {
            out.push_str(&format_field("sql", sql));
        }
    }
    Ok(out)
}

fn auth_event(receipt: &Value) -> Option<String> {
    event_values(receipt, "octra.sqlite.auth")
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
                .map(value_to_string)
                .collect::<Vec<_>>()
                .join(", ")
        })
}

pub(crate) fn print_json(value: &Value) -> Result<()> {
    print!("{}", format_json(value)?);
    Ok(())
}

pub(crate) fn format_json(value: &Value) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}

pub(crate) fn write_text(output: Option<&Path>, text: &str) -> Result<()> {
    if let Some(path) = output {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("writing {}", path.display()))?;
        file.write_all(text.as_bytes())?;
    } else {
        print!("{text}");
    }
    Ok(())
}

pub(crate) fn print_result(value: &Value, mode: OutputMode, headers: bool) -> Result<()> {
    print!("{}", format_result(value, mode, headers)?);
    Ok(())
}

pub(crate) fn format_result(value: &Value, mode: OutputMode, headers: bool) -> Result<String> {
    if mode == OutputMode::Json || value.get("columns").is_none() || value.get("rows").is_none() {
        return format_json(value);
    }
    let columns: Vec<String> = value
        .get("columns")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
        .iter()
        .map(value_to_string)
        .collect();
    let rows: Vec<Vec<String>> = value
        .get("rows")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(Value::as_array)
        .map(|row| row.iter().map(value_to_string).collect())
        .collect();
    match mode {
        OutputMode::Table => Ok(format_table(&columns, &rows, headers)),
        OutputMode::List => {
            let mut out = String::new();
            if headers {
                out.push_str(&columns.join("|"));
                out.push('\n');
            }
            for row in rows {
                out.push_str(&row.join("|"));
                out.push('\n');
            }
            Ok(out)
        }
        OutputMode::Line => {
            let mut out = String::new();
            for row in rows {
                for (idx, value) in row.iter().enumerate() {
                    let name = columns.get(idx).map(String::as_str).unwrap_or("");
                    out.push_str(&format!("{name} = {value}\n"));
                }
                out.push('\n');
            }
            Ok(out)
        }
        OutputMode::Csv => Ok(format_csv(&columns, &rows, headers)),
        OutputMode::Json => format_json(value),
    }
}

fn format_table(columns: &[String], rows: &[Vec<String>], headers: bool) -> String {
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    for row in rows {
        for (idx, value) in row.iter().enumerate() {
            if idx >= widths.len() {
                widths.push(0);
            }
            widths[idx] = widths[idx].max(value.len());
        }
    }
    if widths.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    push_border(&mut out, &widths);
    if headers {
        push_row(&mut out, columns, &widths);
        push_border(&mut out, &widths);
    }
    for row in rows {
        push_row(&mut out, row, &widths);
    }
    push_border(&mut out, &widths);
    out
}

fn push_border(out: &mut String, widths: &[usize]) {
    let mut border = String::new();
    border.push('+');
    for width in widths {
        border.push_str(&"-".repeat(*width + 2));
        border.push('+');
    }
    out.push_str(&dim(border));
    out.push('\n');
}

fn push_row(out: &mut String, values: &[String], widths: &[usize]) {
    out.push_str(&dim("|"));
    for (idx, width) in widths.iter().enumerate() {
        let value = values.get(idx).map(String::as_str).unwrap_or("");
        out.push(' ');
        out.push_str(&format!("{value:<width$}", width = *width));
        out.push(' ');
        out.push_str(&dim("|"));
    }
    out.push('\n');
}

fn format_csv(columns: &[String], rows: &[Vec<String>], headers: bool) -> String {
    let mut out = String::new();
    if headers {
        push_csv_row(&mut out, columns);
    }
    for row in rows {
        push_csv_row(&mut out, row);
    }
    out
}

fn push_csv_row(out: &mut String, values: &[String]) {
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        let needs_quote = value.contains([',', '"', '\n', '\r']);
        if needs_quote {
            out.push('"');
            out.push_str(&value.replace('"', "\"\""));
            out.push('"');
        } else {
            out.push_str(value);
        }
    }
    out.push('\n');
}

pub(crate) fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("blob") => {
            format!(
                "<blob:{}>",
                map.get("base64").and_then(Value::as_str).unwrap_or("")
            )
        }
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exec_result_is_compact_and_keeps_explorer_fields() {
        let result = json!({
            "circle": "octCircle",
            "circle_url": "https://devnet.octrascan.io/address.html?addr=octCircle",
            "wallet": "octWallet",
            "tx_hash": "abc123",
            "tx_url": "https://devnet.octrascan.io/tx.html?hash=abc123",
            "receipt": {
                "contract": "octCircle",
                "success": true,
                "events": [{
                    "event": "octra.sqlite.exec",
                    "values": ["sql_fnv1a64:feedface"]
                }]
            },
            "result": {
                "status": "accepted"
            }
        });
        let rendered = format_exec_result(&result).unwrap();
        assert!(rendered.contains("write: confirmed"));
        assert!(rendered.contains("circle: octCircle"));
        assert!(rendered
            .contains("circle_url: https://devnet.octrascan.io/address.html?addr=octCircle"));
        assert!(rendered.contains("wallet: octWallet"));
        assert!(rendered.contains("tx: abc123"));
        assert!(rendered.contains("tx_url: https://devnet.octrascan.io/tx.html?hash=abc123"));
        assert!(rendered.contains("sql: sql_fnv1a64:feedface"));
        assert!(!rendered.contains("\"receipt\""));
    }

    #[test]
    fn exec_result_surfaces_auth_failure_without_raw_json() {
        let result = json!({
            "circle": "octCircle",
            "wallet": "octWallet",
            "tx_hash": "def456",
            "receipt": {
                "success": false,
                "error": "circle_call_failed: wasm export returned 1",
                "events": [{
                    "event": "octra.sqlite.auth",
                    "values": ["auth_not_authorized:auth_denied"]
                }]
            }
        });
        let rendered = format_exec_result(&result).unwrap();
        assert!(rendered.contains("write: rejected"));
        assert!(rendered.contains("error: circle_call_failed: wasm export returned 1"));
        assert!(rendered.contains("auth: auth_not_authorized:auth_denied"));
        assert!(!rendered.contains("\"events\""));
    }

    #[test]
    fn exec_result_surfaces_sql_error_event_without_raw_json() {
        let result = json!({
            "circle": "octCircle",
            "wallet": "octWallet",
            "tx_hash": "ghi789",
            "receipt": {
                "success": true,
                "error": null,
                "events": [{
                    "event": "octra.sqlite.error",
                    "values": ["sqlite_exec_failed:no such table: correction"]
                }]
            }
        });
        let rendered = format_exec_result(&result).unwrap();
        assert!(rendered.contains("write: confirmed"));
        assert!(rendered.contains("sql_error: sqlite_exec_failed:no such table: correction"));
        assert!(!rendered.contains("\"events\""));
    }

    #[test]
    fn field_formatter_wraps_long_values_without_truncating() {
        let value = "https://devnet.octrascan.io/address.html?addr=octLongCircleId";
        let rendered = format_field_with_width("explorer", value, 40);
        assert_eq!(
            rendered,
            "explorer:\n  https://devnet.octrascan.io/address.html?addr=octLongCircleId\n"
        );
    }

    #[test]
    fn field_formatter_keeps_short_values_on_one_line() {
        let rendered = format_field_with_width("circle", "octCircle", 40);
        assert_eq!(rendered, "circle: octCircle\n");
    }

    #[test]
    fn status_formatter_wraps_long_values_without_truncating() {
        let value = "29861d38ddad25f5cd2b153bb70cfa6b1b54ebd2532fe931fa1f012b7f39ca9c";
        let rendered = format_status_line_with_width("ok", "wasm sha256", value, 48);
        assert_eq!(
            rendered,
            "ok wasm sha256:\n  29861d38ddad25f5cd2b153bb70cfa6b1b54ebd2532fe931fa1f012b7f39ca9c\n"
        );
    }
}
