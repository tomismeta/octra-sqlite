use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::client::low_level::{auth_info, exec_sql, exec_sql_bootstrap_owner, view, Session};

use super::BackupSummary;

pub(super) const MAX_SQL_TEXT_BYTES: usize = 8_191;
pub(super) const SQL_BATCH_TARGET_BYTES: usize = 7_500;
const BACKUP_RETRIES: usize = 2;

pub(super) struct SqlScriptExecution {
    pub statements: usize,
    pub batches: usize,
    pub results: Vec<Value>,
}

pub(super) struct BootstrapOwnerSqlScriptExecution {
    pub execution: SqlScriptExecution,
    pub post_auth_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SqlScriptPlan {
    pub source_bytes: usize,
    pub total_statements: usize,
    pub executable_statements: usize,
    pub skipped_statements: usize,
    pub batches: usize,
    pub max_statement_bytes: usize,
    pub max_payload_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SqlBatchProgress {
    pub batch_index: usize,
    pub total_batches: usize,
    pub start_statement: usize,
    pub end_statement: usize,
    pub statements: usize,
    pub bytes: usize,
}

struct ScriptStatement {
    index: usize,
    sql: String,
}

struct ScriptBatch {
    sql: String,
    start_statement: usize,
    end_statement: usize,
    statements: usize,
    bytes: usize,
}

pub(super) fn backup_database(session: &Session, path: &Path) -> Result<BackupSummary> {
    let mut last_error = None;
    for _ in 0..=BACKUP_RETRIES {
        match backup_database_once(session, path) {
            Ok(summary) => return Ok(summary),
            Err(error) if backup_generation_changed(&error) => last_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("database changed during backup; retry")))
}

fn backup_database_once(session: &Session, path: &Path) -> Result<BackupSummary> {
    let storage = view(session, "storage_info", vec![])?;
    let generation = json_u64(&storage, "generation")?;
    let page_count = json_u64(&storage, "page_count")?;
    let file_bytes = json_u64(&storage, "file_bytes")?;
    if page_count == 0 || file_bytes == 0 {
        bail!("database has no SQLite pages to back up");
    }

    let tmp_path = backup_temp_path(path);
    let _cleanup = TempPathCleanup(tmp_path.clone());
    if let Some(parent) = tmp_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file =
        fs::File::create(&tmp_path).with_context(|| format!("creating {}", tmp_path.display()))?;
    let mut hasher = Sha256::new();
    let mut hashed_bytes = 0u64;
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
        let hash_len = bytes
            .len()
            .min(file_bytes.saturating_sub(hashed_bytes) as usize);
        hasher.update(&bytes[..hash_len]);
        hashed_bytes += hash_len as u64;
        page += chunk_pages;
    }
    if hashed_bytes != file_bytes {
        bail!("backup wrote {hashed_bytes} bytes into hash; expected {file_bytes}");
    }
    file.set_len(file_bytes)?;
    file.flush()?;
    drop(file);
    fs::rename(&tmp_path, path)
        .with_context(|| format!("moving {} to {}", tmp_path.display(), path.display()))?;
    let file_hash = hex::encode(hasher.finalize());
    Ok(BackupSummary {
        path: path.to_path_buf(),
        bytes: file_bytes,
        pages: page_count,
        generation,
        sha256: file_hash,
    })
}

fn backup_generation_changed(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains("backup_generation_changed"))
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
        let error = chunk
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("backup_failed");
        let detail = chunk
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or("backup chunk failed");
        if error == "backup_generation_changed" {
            bail!("{error}: database changed during backup; retry");
        }
        bail!("{error}: {detail}");
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
    Ok(execute_sql_script_with_progress(session, sql, false, |_| {})?.statements)
}

pub(super) fn submit_sql_script_no_wait(
    session: &Session,
    sql: &str,
) -> Result<SqlScriptExecution> {
    execute_sql_script_with_progress(session, sql, true, |_| {})
}

pub(super) fn plan_sql_script(sql: &str) -> Result<SqlScriptPlan> {
    let statements = planned_statements(sql)?;
    let batches = script_batches(&statements)?;
    Ok(SqlScriptPlan {
        source_bytes: sql.len(),
        total_statements: split_sql_statements(sql).len(),
        executable_statements: statements.len(),
        skipped_statements: split_sql_statements(sql)
            .iter()
            .filter(|statement| should_skip_import_wrapper(statement))
            .count(),
        batches: batches.len(),
        max_statement_bytes: statements
            .iter()
            .map(|statement| statement.sql.len())
            .max()
            .unwrap_or(0),
        max_payload_bytes: batches.iter().map(|batch| batch.bytes).max().unwrap_or(0),
    })
}

pub(super) fn execute_sql_script_with_progress(
    session: &Session,
    sql: &str,
    no_wait: bool,
    mut progress: impl FnMut(SqlBatchProgress),
) -> Result<SqlScriptExecution> {
    execute_sql_script_with_submitter(session, sql, no_wait, &mut progress)
}

pub(super) fn execute_sql_script_with_bootstrap_owner_progress(
    session: &Session,
    sql: &str,
    db_id: &str,
    owner_pubkey: &str,
    mut progress: impl FnMut(SqlBatchProgress),
) -> Result<BootstrapOwnerSqlScriptExecution> {
    execute_sql_script_with_bootstrap_owner_submitter(
        session,
        sql,
        db_id,
        owner_pubkey,
        &mut progress,
    )
}

fn execute_sql_script_with_submitter(
    session: &Session,
    sql: &str,
    no_wait: bool,
    progress: &mut impl FnMut(SqlBatchProgress),
) -> Result<SqlScriptExecution> {
    let statements = planned_statements(sql)?;
    let batches = script_batches(&statements)?;
    if batches.is_empty() {
        return Ok(SqlScriptExecution {
            statements: 0,
            batches: 0,
            results: Vec::new(),
        });
    }
    let mut results = Vec::new();
    let total_batches = batches.len();
    let mut executed = 0usize;
    for (offset, batch) in batches.iter().enumerate() {
        let progress_event = SqlBatchProgress {
            batch_index: offset + 1,
            total_batches,
            start_statement: batch.start_statement,
            end_statement: batch.end_statement,
            statements: batch.statements,
            bytes: batch.bytes,
        };
        progress(progress_event.clone());
        let result = exec_sql(session, &batch.sql, no_wait).with_context(|| {
            format!(
                "executing SQL script batch {} of {}; statements {}..{}",
                offset + 1,
                total_batches,
                batch.start_statement,
                batch.end_statement
            )
        })?;
        results.push(result);
        executed += batch.statements;
    }
    Ok(SqlScriptExecution {
        statements: executed,
        batches: total_batches,
        results,
    })
}

fn execute_sql_script_with_bootstrap_owner_submitter(
    session: &Session,
    sql: &str,
    db_id: &str,
    owner_pubkey: &str,
    progress: &mut impl FnMut(SqlBatchProgress),
) -> Result<BootstrapOwnerSqlScriptExecution> {
    let statements = planned_statements(sql)?;
    let batches = script_batches(&statements)?;
    if batches.is_empty() {
        return Ok(BootstrapOwnerSqlScriptExecution {
            execution: SqlScriptExecution {
                statements: 0,
                batches: 0,
                results: Vec::new(),
            },
            post_auth_error: None,
        });
    }

    let mut results = Vec::new();
    let total_batches = batches.len();
    let mut executed = 0usize;
    for (offset, batch) in batches.iter().enumerate() {
        let progress_event = SqlBatchProgress {
            batch_index: offset + 1,
            total_batches,
            start_statement: batch.start_statement,
            end_statement: batch.end_statement,
            statements: batch.statements,
            bytes: batch.bytes,
        };
        progress(progress_event);
        let result = if offset == 0 {
            exec_sql_bootstrap_owner(session, &batch.sql, db_id, owner_pubkey)
        } else {
            exec_sql(session, &batch.sql, false)
        }
        .with_context(|| {
            format!(
                "executing SQL script batch {} of {}; statements {}..{}",
                offset + 1,
                total_batches,
                batch.start_statement,
                batch.end_statement
            )
        })?;
        results.push(result);
        executed += batch.statements;
        if offset == 0 {
            if let Err(error) = auth_info(session) {
                return Ok(BootstrapOwnerSqlScriptExecution {
                    execution: SqlScriptExecution {
                        statements: executed,
                        batches: offset + 1,
                        results,
                    },
                    post_auth_error: Some(format!("{error:#}")),
                });
            }
        }
    }
    Ok(BootstrapOwnerSqlScriptExecution {
        execution: SqlScriptExecution {
            statements: executed,
            batches: total_batches,
            results,
        },
        post_auth_error: None,
    })
}

fn ensure_sql_statement_size(statement: &str) -> Result<()> {
    ensure_sql_len("SQL statement", statement.len())
}

pub(super) fn ensure_sql_text_fits(sql: &str) -> Result<()> {
    ensure_sql_statement_size(sql)
}

fn ensure_exec_payload_size(sql: &str) -> Result<()> {
    ensure_sql_len("SQL payload", sql.len())
}

fn ensure_sql_len(label: &str, len: usize) -> Result<()> {
    if len > MAX_SQL_TEXT_BYTES {
        bail!(
            "{label} is {len} bytes; Octra SQLite accepts at most {MAX_SQL_TEXT_BYTES} bytes per statement"
        );
    }
    Ok(())
}

pub(super) fn should_skip_import_wrapper(statement: &str) -> bool {
    if should_skip_foreign_keys_pragma(statement) {
        return true;
    }
    let trimmed = normalized_sql_statement(statement);
    trimmed == "begin transaction" || trimmed == "commit"
}

fn should_skip_foreign_keys_pragma(statement: &str) -> bool {
    let trimmed = normalized_sql_statement(statement);
    trimmed.starts_with("pragma foreign_keys")
}

fn normalized_sql_statement(statement: &str) -> String {
    statement
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_ascii_lowercase()
}

fn ensure_supported_restore_statement(statement: &str) -> Result<()> {
    let trimmed = normalized_sql_statement(statement);
    let unsupported = trimmed == "rollback"
        || trimmed.starts_with("rollback ")
        || trimmed == "end"
        || trimmed.starts_with("end ")
        || (trimmed.starts_with("commit ") && trimmed != "commit")
        || trimmed == "savepoint"
        || trimmed.starts_with("savepoint ")
        || trimmed == "release"
        || trimmed.starts_with("release ")
        || (trimmed.starts_with("begin") && trimmed != "begin transaction");
    if unsupported {
        bail!(
            "transactions_not_supported: restore only strips SQLite dump wrappers (BEGIN TRANSACTION, COMMIT, PRAGMA foreign_keys); unsupported transaction control statement: {statement}"
        );
    }
    Ok(())
}

#[cfg(test)]
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

fn planned_statements(sql: &str) -> Result<Vec<ScriptStatement>> {
    let mut statements = Vec::new();
    for (offset, statement) in split_sql_statements(sql).into_iter().enumerate() {
        ensure_sql_statement_size(&statement)
            .with_context(|| format!("statement {}", offset + 1))?;
        ensure_supported_restore_statement(&statement)
            .with_context(|| format!("statement {}", offset + 1))?;
        if should_skip_import_wrapper(&statement) {
            continue;
        }
        statements.push(ScriptStatement {
            index: offset + 1,
            sql: statement,
        });
    }
    Ok(statements)
}

fn script_batches(statements: &[ScriptStatement]) -> Result<Vec<ScriptBatch>> {
    let mut batches = Vec::new();
    let mut batch = String::new();
    let mut start_statement = 0usize;
    let mut end_statement = 0usize;
    let mut batch_statements = 0usize;

    for statement in statements {
        let rendered = render_statement_for_batch(&statement.sql);
        if !batch.is_empty() && batch.len() + rendered.len() >= SQL_BATCH_TARGET_BYTES {
            push_script_batch(
                &mut batches,
                std::mem::take(&mut batch),
                start_statement,
                end_statement,
                batch_statements,
            )?;
            start_statement = 0;
            end_statement = 0;
            batch_statements = 0;
        }
        if rendered.len() >= SQL_BATCH_TARGET_BYTES {
            push_script_batch(&mut batches, rendered, statement.index, statement.index, 1)?;
            continue;
        }
        if batch.is_empty() {
            start_statement = statement.index;
        }
        end_statement = statement.index;
        batch_statements += 1;
        batch.push_str(&rendered);
    }
    if !batch.trim().is_empty() {
        push_script_batch(
            &mut batches,
            batch,
            start_statement,
            end_statement,
            batch_statements,
        )?;
    }
    Ok(batches)
}

fn push_script_batch(
    batches: &mut Vec<ScriptBatch>,
    sql: String,
    start_statement: usize,
    end_statement: usize,
    statements: usize,
) -> Result<()> {
    ensure_exec_payload_size(&sql)?;
    batches.push(ScriptBatch {
        bytes: sql.len(),
        sql,
        start_statement,
        end_statement,
        statements,
    });
    Ok(())
}

fn render_statement_for_batch(statement: &str) -> String {
    let trimmed = statement.trim();
    if trimmed.len() >= SQL_BATCH_TARGET_BYTES {
        return trimmed.to_string();
    }
    let mut out = trimmed.to_string();
    if !out.ends_with(';') {
        out.push(';');
    }
    out.push('\n');
    out
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
        ensure_sql_statement_size(&statement)
            .with_context(|| format!("CSV row {} is too large to import", idx + 1))?;
        if !batch.is_empty() && batch.len() + statement.len() + 1 >= SQL_BATCH_TARGET_BYTES {
            ensure_exec_payload_size(&batch)?;
            exec_sql(session, &batch, false)?;
            batch.clear();
        }
        batch.push_str(&statement);
        batch.push('\n');
        inserted += 1;
    }
    if !batch.trim().is_empty() {
        ensure_exec_payload_size(&batch)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command as ProcessCommand;

    #[test]
    fn sql_statement_size_limit_is_explicit() {
        assert!(ensure_sql_statement_size(&"x".repeat(MAX_SQL_TEXT_BYTES)).is_ok());
        let error = ensure_sql_statement_size(&"x".repeat(MAX_SQL_TEXT_BYTES + 1))
            .unwrap_err()
            .to_string();
        assert!(error.contains("8192 bytes"));
        assert!(error.contains("8191 bytes"));
    }

    #[test]
    fn rewritten_sql_script_payload_is_checked() {
        let statements = vec!["x;".to_string(); 3_749];
        let script = sql_script_for_single_exec(&statements);
        assert!(script.len() > MAX_SQL_TEXT_BYTES);
        let error = ensure_exec_payload_size(&script).unwrap_err().to_string();
        assert!(error.contains("SQL payload"));
    }

    #[test]
    fn script_plan_skips_dump_wrappers_and_counts_batches() {
        let sql = "BEGIN TRANSACTION;
create table artist(id integer primary key, name text not null);
insert into artist(name) values ('Monet');
COMMIT;";
        let plan = plan_sql_script(sql).unwrap();
        assert_eq!(plan.total_statements, 4);
        assert_eq!(plan.executable_statements, 2);
        assert_eq!(plan.skipped_statements, 2);
        assert_eq!(plan.batches, 1);
        assert!(plan.max_payload_bytes > 0);
    }

    #[test]
    fn script_plan_rejects_rollback_and_savepoints() {
        let rollback = "BEGIN TRANSACTION;
insert into artist(name) values ('Monet');
ROLLBACK;";
        let error = format!("{:#}", plan_sql_script(rollback).unwrap_err());
        assert!(error.contains("transactions_not_supported"));
        assert!(error.contains("statement 3"));

        let savepoint = "SAVEPOINT load;
insert into artist(name) values ('Monet');
RELEASE load;";
        let error = format!("{:#}", plan_sql_script(savepoint).unwrap_err());
        assert!(error.contains("transactions_not_supported"));
        assert!(error.contains("statement 1"));
    }

    #[test]
    fn script_plan_rejects_bare_begin() {
        let error = format!(
            "{:#}",
            plan_sql_script("BEGIN; insert into artist(name) values ('Monet'); COMMIT;")
                .unwrap_err()
        );
        assert!(error.contains("transactions_not_supported"));
        assert!(error.contains("statement 1"));
    }

    #[test]
    fn script_plan_rejects_commit_transaction() {
        let error = format!(
            "{:#}",
            plan_sql_script(
                "BEGIN TRANSACTION; insert into artist(name) values ('Monet'); COMMIT TRANSACTION;",
            )
            .unwrap_err()
        );
        assert!(error.contains("transactions_not_supported"));
        assert!(error.contains("statement 3"));
    }

    #[test]
    fn splitter_round_trips_real_sqlite_dump_when_available() -> Result<()> {
        if ProcessCommand::new("sqlite3")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("sqlite3 not installed; skipping splitter conformance smoke");
            return Ok(());
        }

        let dir =
            std::env::temp_dir().join(format!("octra-sqlite-splitter-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir)?;
        let source = dir.join("source.sqlite");
        let restored = dir.join("restored.sqlite");
        let seed = "create table person(id integer primary key, name text not null);
create table audit(person_id integer not null, note text not null);
create trigger person_audit after insert on person begin
  insert into audit(person_id, note) values (new.id, 'created; ok');
end;
insert into person(name) values ('Ada; Lovelace'),('Grace Hopper');";

        assert!(ProcessCommand::new("sqlite3")
            .arg(&source)
            .arg(seed)
            .status()?
            .success());
        let dump = ProcessCommand::new("sqlite3")
            .arg(&source)
            .arg(".dump")
            .output()?;
        assert!(dump.status.success());
        let dump = String::from_utf8(dump.stdout)?;
        let statements = split_sql_statements(&dump);
        assert!(statements.iter().any(|sql| sql.contains("CREATE TRIGGER")));
        assert!(statements.iter().any(|sql| sql.contains("created; ok")));

        let split_dump = dir.join("split.sql");
        fs::write(&split_dump, statements.join("\n"))?;
        assert!(ProcessCommand::new("sqlite3")
            .arg(&restored)
            .arg(format!(".read {}", split_dump.display()))
            .status()?
            .success());
        let integrity = ProcessCommand::new("sqlite3")
            .arg(&restored)
            .arg("pragma integrity_check;")
            .output()?;
        assert!(integrity.status.success());
        assert_eq!(String::from_utf8(integrity.stdout)?.trim(), "ok");
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn csv_row_with_newline_cannot_cross_payload_limit() {
        let statement = "x".repeat(MAX_SQL_TEXT_BYTES);
        assert!(ensure_sql_statement_size(&statement).is_ok());
        let payload = format!("{statement}\n");
        let error = ensure_exec_payload_size(&payload).unwrap_err().to_string();
        assert!(error.contains("8192 bytes"));
    }

    #[test]
    fn backup_generation_change_has_clear_error() {
        let chunk = serde_json::json!({
            "ok": false,
            "error": "backup_generation_changed",
            "detail": "database generation changed during backup"
        });
        let error = ensure_backup_chunk_matches(&chunk, 1, 1, 1, 1, 4096)
            .unwrap_err()
            .to_string();
        assert!(error.contains("backup_generation_changed"));
        assert!(error.contains("database changed during backup"));
        assert!(backup_generation_changed(&anyhow!(error)));
    }

    #[test]
    fn backup_chunk_errors_hide_raw_json() {
        let chunk = serde_json::json!({
            "ok": false,
            "error": "backup_bad_range",
            "detail": "backup_chunk page range is out of bounds"
        });
        let error = ensure_backup_chunk_matches(&chunk, 1, 1, 1, 1, 4096)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "backup_bad_range: backup_chunk page range is out of bounds"
        );
        assert!(!error.contains("{"));
    }
}
