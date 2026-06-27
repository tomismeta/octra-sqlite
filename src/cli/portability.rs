use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::client::low_level::{exec_sql, view, Session};

use super::BackupSummary;

pub(super) fn backup_database(session: &Session, path: &Path) -> Result<BackupSummary> {
    let storage = view(session, "storage_info", vec![])?;
    let generation = json_u64(&storage, "generation")?;
    let page_count = json_u64(&storage, "page_count")?;
    let file_bytes = json_u64(&storage, "file_bytes")?;
    if page_count == 0 || file_bytes == 0 {
        bail!("database has no SQLite pages to back up");
    }

    let tmp_path = backup_temp_path(path);
    if let Some(parent) = tmp_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file =
        fs::File::create(&tmp_path).with_context(|| format!("creating {}", tmp_path.display()))?;
    let mut page = 1u64;
    while page <= page_count {
        let remaining = page_count - page + 1;
        let chunk_pages = remaining.min(8);
        let chunk = view(
            session,
            "backup_chunk",
            vec![
                Value::String(generation.to_string()),
                Value::String(page.to_string()),
                Value::String(chunk_pages.to_string()),
            ],
        )?;
        ensure_backup_chunk_matches(
            &chunk,
            generation,
            page,
            chunk_pages,
            page_count,
            file_bytes,
        )?;
        let encoded = chunk
            .get("data_b64")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("backup chunk missing data_b64"))?;
        let bytes = general_purpose::STANDARD
            .decode(encoded)
            .context("decoding backup chunk")?;
        let expected_len = (chunk_pages as usize) * 4096;
        if bytes.len() != expected_len {
            bail!(
                "backup chunk returned {} bytes; expected {expected_len}",
                bytes.len()
            );
        }
        file.write_all(&bytes)?;
        page += chunk_pages;
    }
    file.set_len(file_bytes)?;
    file.flush()?;
    drop(file);
    fs::rename(&tmp_path, path)
        .with_context(|| format!("moving {} to {}", tmp_path.display(), path.display()))?;
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let file_hash = hex::encode(hasher.finalize());
    Ok(BackupSummary {
        path: path.to_path_buf(),
        bytes: file_bytes,
        pages: page_count,
        generation,
        sha256: file_hash,
    })
}

fn backup_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "octra-sqlite-backup.sqlite".to_string());
    path.with_file_name(format!(".{file_name}.tmp.{}", std::process::id()))
}

fn ensure_backup_chunk_matches(
    chunk: &Value,
    generation: u64,
    start_page: u64,
    chunk_pages: u64,
    page_count: u64,
    file_bytes: u64,
) -> Result<()> {
    if chunk.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("backup chunk failed: {chunk}");
    }
    for (key, expected) in [
        ("generation", generation),
        ("start_page", start_page),
        ("chunk_pages", chunk_pages),
        ("page_count", page_count),
        ("file_bytes", file_bytes),
    ] {
        let actual = json_u64(chunk, key)?;
        if actual != expected {
            bail!("backup chunk {key} changed from {expected} to {actual}");
        }
    }
    Ok(())
}

fn json_u64(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("missing numeric {key}"))
}

pub(super) fn run_local_sqlite_integrity(path: &Path) -> Result<String> {
    let output = ProcessCommand::new("sqlite3")
        .arg(path)
        .arg("pragma integrity_check;")
        .output()
        .with_context(|| "running sqlite3 integrity_check")?;
    if !output.status.success() {
        bail!(
            "sqlite3 integrity_check failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout != "ok" {
        bail!("sqlite3 integrity_check returned {stdout}");
    }
    Ok(stdout)
}

pub(super) fn dump_database(session: &Session, objects: &[String]) -> Result<String> {
    run_sqlite_snapshot_dot_command(session, ".dump", objects)
}

pub(super) fn fullschema_database(session: &Session) -> Result<String> {
    run_sqlite_snapshot_dot_command(session, ".fullschema", &[])
}

fn run_sqlite_snapshot_dot_command(
    session: &Session,
    command: &str,
    objects: &[String],
) -> Result<String> {
    let path = sqlite_snapshot_temp_path(command);
    let _cleanup = TempPathCleanup(path.clone());
    backup_database(session, &path)?;
    let mut dot_command = command.to_string();
    for object in objects {
        dot_command.push(' ');
        dot_command.push_str(&sqlite_dot_argument(object)?);
    }
    let output = ProcessCommand::new("sqlite3")
        .arg(&path)
        .arg(&dot_command)
        .output()
        .with_context(|| "running sqlite3 against exported backup")?;
    if !output.status.success() {
        bail!(
            "sqlite3 {command} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn sqlite_snapshot_temp_path(label: &str) -> PathBuf {
    let clean_label = label
        .trim_start_matches('.')
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>();
    std::env::temp_dir().join(format!(
        "octra-sqlite-{clean_label}-{}-{}.sqlite",
        std::process::id(),
        now_millis()
    ))
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

struct TempPathCleanup(PathBuf);

impl Drop for TempPathCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn sqlite_dot_argument(value: &str) -> Result<String> {
    if value.contains(['\0', '\n', '\r', '\'']) {
        bail!("sqlite object names for .dump cannot contain quotes or control characters");
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(value.to_string())
    } else {
        Ok(format!("'{value}'"))
    }
}

pub(super) fn sql_string_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub(super) fn execute_sql_script(session: &Session, sql: &str) -> Result<usize> {
    let statements = split_sql_statements(sql);
    if statements.is_empty() {
        return Ok(0);
    }
    if sql.trim().len() < 7_500 {
        let script = sql_script_for_single_exec(&statements);
        if script.trim().is_empty() {
            return Ok(0);
        }
        exec_sql(session, &script, false)?;
        return Ok(statements
            .iter()
            .filter(|statement| !should_skip_import_wrapper(statement))
            .count());
    }
    let mut batch = String::new();
    let mut executed = 0usize;
    for statement in statements {
        if should_skip_import_wrapper(&statement) {
            continue;
        }
        let candidate_len = batch.len() + statement.len() + 1;
        if !batch.is_empty() && candidate_len >= 7_500 {
            exec_sql(session, &batch, false).context(
                "executing SQL script batch; large .read files are applied in batches on Octra",
            )?;
            batch.clear();
        }
        if statement.len() >= 7_500 {
            exec_sql(session, &statement, false).context(
                "executing SQL script statement; large .read files are applied in batches on Octra",
            )?;
            executed += 1;
            continue;
        }
        batch.push_str(&statement);
        if !batch.ends_with(';') {
            batch.push(';');
        }
        batch.push('\n');
        executed += 1;
    }
    if !batch.trim().is_empty() {
        exec_sql(session, &batch, false).context(
            "executing SQL script batch; large .read files are applied in batches on Octra",
        )?;
    }
    Ok(executed)
}

pub(super) fn should_skip_import_wrapper(statement: &str) -> bool {
    if should_skip_foreign_keys_pragma(statement) {
        return true;
    }
    let trimmed = statement
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_ascii_lowercase();
    trimmed == "begin"
        || trimmed == "begin transaction"
        || trimmed == "commit"
        || trimmed == "end"
        || trimmed == "rollback"
}

fn should_skip_foreign_keys_pragma(statement: &str) -> bool {
    let trimmed = statement
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_ascii_lowercase();
    trimmed.starts_with("pragma foreign_keys")
}

pub(super) fn sql_script_for_single_exec(statements: &[String]) -> String {
    let mut script = String::new();
    for statement in statements {
        if should_skip_import_wrapper(statement) {
            continue;
        }
        script.push_str(statement.trim());
        if !statement.trim_end().ends_with(';') {
            script.push(';');
        }
        script.push('\n');
    }
    script
}

pub(super) fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut start = 0usize;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backtick = false;
    let mut in_bracket = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut chars = sql.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if in_line_comment {
            if ch == '\n' || ch == '\r' {
                in_line_comment = false;
            }
            continue;
        }
        if in_block_comment {
            if ch == '*' && chars.peek().is_some_and(|(_, next)| *next == '/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }
        if in_bracket {
            if ch == ']' {
                if chars.peek().is_some_and(|(_, next)| *next == ']') {
                    chars.next();
                } else {
                    in_bracket = false;
                }
            }
            continue;
        }
        if in_backtick {
            if ch == '`' {
                if chars.peek().is_some_and(|(_, next)| *next == '`') {
                    chars.next();
                } else {
                    in_backtick = false;
                }
            }
            continue;
        }
        match ch {
            '\'' if !in_double_quote => {
                if in_single_quote && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    in_single_quote = !in_single_quote;
                }
            }
            '"' if !in_single_quote => {
                if in_double_quote && chars.peek().is_some_and(|(_, next)| *next == '"') {
                    chars.next();
                } else {
                    in_double_quote = !in_double_quote;
                }
            }
            '`' if !in_single_quote && !in_double_quote => in_backtick = true,
            '[' if !in_single_quote && !in_double_quote => in_bracket = true,
            '-' if !in_single_quote
                && !in_double_quote
                && chars.peek().is_some_and(|(_, next)| *next == '-') =>
            {
                chars.next();
                in_line_comment = true;
            }
            '/' if !in_single_quote
                && !in_double_quote
                && chars.peek().is_some_and(|(_, next)| *next == '*') =>
            {
                chars.next();
                in_block_comment = true;
            }
            ';' if !in_single_quote && !in_double_quote => {
                let statement = sql[start..=index].trim();
                if !statement.is_empty() {
                    if create_trigger_statement(statement)
                        && !trigger_statement_complete_at_semicolon(statement)
                    {
                        continue;
                    }
                    statements.push(statement.to_string());
                }
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = sql[start..].trim();
    if !tail.is_empty() {
        statements.push(tail.to_string());
    }
    statements
}

fn create_trigger_statement(statement: &str) -> bool {
    let tokens = sql_keyword_tokens(statement);
    if tokens.first().map(String::as_str) != Some("create") {
        return false;
    }
    let mut index = 1usize;
    if tokens
        .get(index)
        .is_some_and(|token| token == "temp" || token == "temporary")
    {
        index += 1;
    }
    tokens.get(index).is_some_and(|token| token == "trigger")
}

fn trigger_statement_complete_at_semicolon(statement: &str) -> bool {
    sql_tail_tokens_since_last_semicolon(statement) == ["end"]
}

fn sql_keyword_tokens(sql: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    scan_sql_tokens(sql, |token| tokens.push(token.to_ascii_lowercase()));
    tokens
}

fn sql_tail_tokens_since_last_semicolon(sql: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut tail = Vec::new();
    scan_sql_tokens(sql, |token| {
        if token == ";" {
            tail = std::mem::take(&mut tokens);
        } else {
            tokens.push(token.to_ascii_lowercase());
        }
    });
    if !tokens.is_empty() {
        tokens
    } else {
        tail
    }
}

fn scan_sql_tokens(mut sql: &str, mut visit: impl FnMut(&str)) {
    while !sql.is_empty() {
        let Some((offset, ch)) = sql.char_indices().next() else {
            break;
        };
        debug_assert_eq!(offset, 0);
        if ch.is_whitespace() {
            sql = &sql[ch.len_utf8()..];
            continue;
        }
        if sql.starts_with("--") {
            if let Some(end) = sql.find(['\n', '\r']) {
                sql = &sql[end..];
            } else {
                break;
            }
            continue;
        }
        if sql.starts_with("/*") {
            if let Some(end) = sql.find("*/") {
                sql = &sql[end + 2..];
            } else {
                break;
            }
            continue;
        }
        if ch == '\'' || ch == '"' || ch == '`' {
            sql = skip_quoted_sql(sql, ch);
            continue;
        }
        if ch == '[' {
            sql = skip_bracket_quoted_sql(sql);
            continue;
        }
        if ch == ';' {
            visit(";");
            sql = &sql[1..];
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let end = sql
                .char_indices()
                .find(|(_, token_ch)| !(token_ch.is_ascii_alphanumeric() || *token_ch == '_'))
                .map(|(idx, _)| idx)
                .unwrap_or(sql.len());
            visit(&sql[..end]);
            sql = &sql[end..];
            continue;
        }
        sql = &sql[ch.len_utf8()..];
    }
}

fn skip_quoted_sql(sql: &str, quote: char) -> &str {
    let mut chars = sql.char_indices();
    chars.next();
    while let Some((idx, ch)) = chars.next() {
        if ch == quote {
            if chars.clone().next().is_some_and(|(_, next)| next == quote) {
                chars.next();
            } else {
                let end = idx + ch.len_utf8();
                return &sql[end..];
            }
        }
    }
    ""
}

fn skip_bracket_quoted_sql(sql: &str) -> &str {
    let mut chars = sql.char_indices();
    chars.next();
    while let Some((idx, ch)) = chars.next() {
        if ch == ']' {
            if chars.clone().next().is_some_and(|(_, next)| next == ']') {
                chars.next();
            } else {
                return &sql[idx + 1..];
            }
        }
    }
    ""
}

pub(super) fn import_csv(
    session: &Session,
    path: &Path,
    table: &str,
    skip: usize,
) -> Result<usize> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut inserted = 0usize;
    let mut batch = String::new();
    for (idx, row) in parse_csv_records(&text)?.into_iter().enumerate() {
        if idx < skip {
            continue;
        }
        let statement = format!(
            "insert into {} values({});",
            quote_identifier(table),
            row.iter()
                .map(|value| sql_string_literal(value))
                .collect::<Vec<_>>()
                .join(",")
        );
        if !batch.is_empty() && batch.len() + statement.len() + 1 >= 7_500 {
            exec_sql(session, &batch, false)?;
            batch.clear();
        }
        batch.push_str(&statement);
        batch.push('\n');
        inserted += 1;
    }
    if !batch.trim().is_empty() {
        exec_sql(session, &batch, false)?;
    }
    Ok(inserted)
}

fn parse_csv_records(text: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek().is_some_and(|next| *next == '"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => in_quotes = true,
            ',' => {
                row.push(std::mem::take(&mut field));
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\r' => {
                if chars.peek().is_some_and(|next| *next == '\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(ch),
        }
    }
    if in_quotes {
        bail!("unterminated CSV quote");
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}
