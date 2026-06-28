use anyhow::{anyhow, bail, Context, Result};
use rustyline::error::ReadlineError;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::client::{
    config_path,
    low_level::{program_info, query_typed, view, Session},
};

use super::output::{dim, format_json, format_result, strong, write_text, OutputMode};
use super::portability::{
    backup_database, dump_database, execute_sql_script, fullschema_database, import_csv,
    sql_string_literal,
};
use super::{
    format_schema_result, linked_circle, print_field, run_one_sql_to, verify, BackupSummary,
};

struct ShellState {
    session: Session,
    mode: OutputMode,
    headers: bool,
    timer: bool,
    output: Option<PathBuf>,
    once_output: Option<PathBuf>,
}

pub(super) fn run_dot_command(
    session: Session,
    mode: OutputMode,
    headers: bool,
    output: Option<&Path>,
    line: &str,
) -> Result<bool> {
    let mut state = ShellState {
        session,
        mode,
        headers,
        timer: false,
        output: output.map(Path::to_path_buf),
        once_output: None,
    };
    handle_dot_command(&mut state, line)
}

pub(super) fn run_shell(session: Session, mode: OutputMode) -> Result<()> {
    println!(
        "{}",
        strong(format!("SQLite on Octra ({})", session.target().network))
    );
    print_field(
        "circle",
        linked_circle(&session.target().network, &session.target().circle),
    );
    print_field("wallet", session.caller());
    println!("{}", dim("type .help for usage"));
    let mut state = ShellState {
        session,
        mode,
        headers: true,
        timer: false,
        output: None,
        once_output: None,
    };
    let mut editor = rustyline::DefaultEditor::new()?;
    let history_path = shell_history_path()?;
    let _ = editor.load_history(&history_path);
    let mut buffer = String::new();
    loop {
        let prompt = if buffer.trim().is_empty() {
            "sqlite> "
        } else {
            "   ...> "
        };
        let line = match editor.readline(prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => break,
            Err(error) => return Err(error.into()),
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let _ = editor.add_history_entry(line.as_str());
        if buffer.trim().is_empty() && trimmed.starts_with('.') {
            if handle_dot_command(&mut state, trimmed)? {
                break;
            }
            continue;
        }
        buffer.push_str(&line);
        buffer.push('\n');
        if trimmed.ends_with(';') {
            let sql = buffer.trim().to_string();
            buffer.clear();
            let started = Instant::now();
            let output = take_command_output(&mut state);
            if let Err(error) = run_one_sql_to(
                &state.session,
                &sql,
                state.mode,
                state.headers,
                output.as_deref(),
                false,
            ) {
                eprintln!("error: {error:#}");
            }
            if state.timer {
                println!("Run Time: real {:.3}", started.elapsed().as_secs_f64());
            }
        }
    }
    let _ = editor.save_history(&history_path);
    Ok(())
}

fn shell_history_path() -> Result<PathBuf> {
    let path = config_path()?.with_file_name("sqlite_history");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(path)
}

fn handle_dot_command(state: &mut ShellState, line: &str) -> Result<bool> {
    let parts = parse_dot_parts(line)?;
    let cmd = parts.first().map(String::as_str).unwrap_or("");
    let args = &parts[1..];
    match cmd {
        ".quit" | ".exit" => return Ok(true),
        ".help" => print_help(),
        ".mode" => {
            let mode = args
                .first()
                .map(String::as_str)
                .ok_or_else(|| anyhow!("usage: .mode box|table|list|json|line|csv"))?;
            state.mode = match mode {
                "box" | "table" => OutputMode::Table,
                "list" => OutputMode::List,
                "json" => OutputMode::Json,
                "line" => OutputMode::Line,
                "csv" => OutputMode::Csv,
                _ => bail!("unknown mode {mode}"),
            };
        }
        ".headers" => {
            let value = args
                .first()
                .map(String::as_str)
                .ok_or_else(|| anyhow!("usage: .headers on|off"))?;
            state.headers = match value {
                "on" => true,
                "off" => false,
                _ => bail!("usage: .headers on|off"),
            };
        }
        ".tables" => {
            let result = query_typed(
                &state.session,
                "select name from sqlite_master where type='table' order by name;",
            )?;
            let output = take_command_output(state);
            write_text(
                output.as_deref(),
                &format_result(&result, state.mode, state.headers)?,
            )?;
        }
        ".schema" => {
            let sql = if let Some(name) = args.first() {
                format!(
                    "select type, name, sql from sqlite_master where sql is not null and (name = {} or tbl_name = {}) order by type, name;",
                    sql_string_literal(name),
                    sql_string_literal(name)
                )
            } else {
                "select type, name, sql from sqlite_master where sql is not null and type in ('table','view','index','trigger') order by type, name;".to_string()
            };
            let result = query_typed(&state.session, &sql)?;
            let output = take_command_output(state);
            write_text(output.as_deref(), &format_schema_result(&result)?)?;
        }
        ".show" => print_shell_show(state),
        ".databases" => print_current_database(&state.session),
        ".storage" => write_text(
            take_command_output(state).as_deref(),
            &format_json(&view(&state.session, "storage_info", vec![])?)?,
        )?,
        ".circle" => write_text(
            take_command_output(state).as_deref(),
            &format_json(&program_info(&state.session)?)?,
        )?,
        ".wallet" => {
            println!("{}", state.session.caller());
            if let Some(path) = state.session.wallet_path() {
                print_field("wallet", path.display().to_string());
            }
        }
        ".verify" => verify(&state.session, None, false, false, false)?,
        ".backup" => {
            let path = backup_path_from_args(args)?;
            let summary = backup_database(&state.session, &path)?;
            print_backup_summary(&summary);
        }
        ".save" => {
            let path = save_path_from_args(args)?;
            let summary = backup_database(&state.session, &path)?;
            print_backup_summary(&summary);
        }
        ".dump" => {
            let output = take_command_output(state);
            write_text(output.as_deref(), &dump_database(&state.session, args)?)?;
        }
        ".read" => {
            let path = args.first().ok_or_else(|| anyhow!("usage: .read FILE"))?;
            reject_shell_pipe_arg(path, ".read")?;
            let sql = fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
            execute_sql_script(&state.session, &sql)?;
        }
        ".import" => {
            let (path, table, skip) = import_args(args)?;
            let inserted = import_csv(&state.session, &path, &table, skip)?;
            print_field("imported", format!("{inserted} rows"));
        }
        ".indexes" => {
            let sql = if let Some(table) = args.first() {
                format!(
                    "select name from sqlite_master where type='index' and tbl_name = {} order by name;",
                    sql_string_literal(table)
                )
            } else {
                "select name from sqlite_master where type='index' order by name;".to_string()
            };
            let result = query_typed(&state.session, &sql)?;
            let output = take_command_output(state);
            write_text(
                output.as_deref(),
                &format_result(&result, state.mode, state.headers)?,
            )?;
        }
        ".fullschema" => {
            let output = take_command_output(state);
            write_text(output.as_deref(), &fullschema_database(&state.session)?)?;
        }
        ".timer" => {
            let value = args
                .first()
                .map(String::as_str)
                .ok_or_else(|| anyhow!("usage: .timer on|off"))?;
            state.timer = match value {
                "on" => true,
                "off" => false,
                _ => bail!("usage: .timer on|off"),
            };
        }
        ".output" => {
            let Some(value) = args.first() else {
                state.output = None;
                return Ok(false);
            };
            reject_shell_pipe_arg(value, ".output")?;
            if value == "stdout" || value == "off" {
                state.output = None;
            } else {
                fs::write(value, "").with_context(|| format!("opening output {value}"))?;
                state.output = Some(PathBuf::from(value));
            }
        }
        ".once" => {
            let value = args.first().ok_or_else(|| anyhow!("usage: .once FILE"))?;
            if value.starts_with('-') {
                bail!(".once options are not supported");
            }
            reject_shell_pipe_arg(value, ".once")?;
            fs::write(value, "").with_context(|| format!("opening output {value}"))?;
            state.once_output = Some(PathBuf::from(value));
        }
        ".open" => {
            let target = args
                .first()
                .map(String::as_str)
                .ok_or_else(|| anyhow!("usage: .open DATABASE"))?;
            state.session = state.session.open_database(target)?;
            print_field(
                "circle",
                linked_circle(
                    &state.session.target().network,
                    &state.session.target().circle,
                ),
            );
        }
        _ => bail!("unknown command {cmd}; try .help"),
    }
    Ok(false)
}

fn print_help() {
    println!("SQLite commands:");
    println!("  .backup ?DB? FILE    back up main database to a SQLite file");
    println!("  .save FILE           save main database to a SQLite file");
    println!("  .dump ?OBJECTS?      render SQL text for the database or table");
    println!("  .import --csv FILE TABLE");
    println!("                      import CSV rows into TABLE");
    println!("  .indexes ?TABLE?     list indexes");
    println!("  .fullschema          show SQLite .fullschema output");
    println!("  .tables              list tables");
    println!("  .schema ?TABLE?      show schema");
    println!("  .databases           show the current main database URI");
    println!("  .open DATABASE       switch database name, Circle ID, or oct:// URI");
    println!("  .read FILE           execute SQL from a file");
    println!("  .mode MODE           MODE is box, table, list, json, line, or csv");
    println!("  .headers on|off      show or hide column headers");
    println!("  .timer on|off        show query timing");
    println!("  .output ?FILE?       redirect output; no FILE restores stdout");
    println!("  .once FILE           redirect the next command only");
    println!("  .show                show shell settings");
    println!("  .quit                exit");
    println!();
    println!("Octra commands:");
    println!("  .storage             show SQLite page storage info");
    println!("  .circle              show Circle program metadata");
    println!("  .wallet              show active wallet address");
    println!("  .verify              verify live Circle SQLite status");
}

pub(super) fn parse_dot_parts(line: &str) -> Result<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = line.trim().chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(ch) = chars.next() {
        if let Some(active) = quote {
            if ch == active {
                if chars.peek().is_some_and(|next| *next == active) {
                    chars.next();
                    current.push(active);
                } else {
                    quote = None;
                }
            } else {
                current.push(ch);
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            ch if ch.is_whitespace() => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if quote.is_some() {
        bail!("unterminated quoted dot-command argument");
    }
    if !current.is_empty() {
        parts.push(current);
    }
    Ok(parts)
}

fn take_command_output(state: &mut ShellState) -> Option<PathBuf> {
    state.once_output.take().or_else(|| state.output.clone())
}

pub(super) fn backup_path_from_args(args: &[String]) -> Result<PathBuf> {
    match args {
        [file] => Ok(PathBuf::from(file)),
        [db, file] if db == "main" => Ok(PathBuf::from(file)),
        [db, _] => bail!("only .backup main FILE is supported; got database {db}"),
        _ => bail!("usage: .backup ?main? FILE"),
    }
}

pub(super) fn save_path_from_args(args: &[String]) -> Result<PathBuf> {
    match args {
        [file] => Ok(PathBuf::from(file)),
        [option, _] if option.starts_with('-') => bail!(".save options are not supported"),
        _ => bail!("usage: .save FILE"),
    }
}

pub(super) fn reject_shell_pipe_arg(value: &str, command: &str) -> Result<()> {
    if value.starts_with('|') {
        bail!("{command} shell pipes are intentionally unsupported");
    }
    Ok(())
}

pub(super) fn import_args(args: &[String]) -> Result<(PathBuf, String, usize)> {
    let mut csv = false;
    let mut skip = 0usize;
    let mut positional = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--csv" => {
                csv = true;
                index += 1;
            }
            "--skip" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("usage: .import --csv --skip N FILE TABLE"))?;
                skip = value.parse::<usize>().context("parsing .import --skip")?;
                index += 2;
            }
            value if value.starts_with('-') => bail!("unsupported .import option {value}"),
            value => {
                positional.push(value.to_string());
                index += 1;
            }
        }
    }
    if !csv {
        bail!("only .import --csv FILE TABLE is supported");
    }
    if positional.len() != 2 {
        bail!("usage: .import --csv [--skip N] FILE TABLE");
    }
    reject_shell_pipe_arg(&positional[0], ".import")?;
    Ok((PathBuf::from(&positional[0]), positional[1].clone(), skip))
}

fn print_backup_summary(summary: &BackupSummary) {
    print_field("backup", summary.path.display().to_string());
    print_field(
        "database",
        format!(
            "{} bytes, {} pages, generation {}",
            summary.bytes, summary.pages, summary.generation
        ),
    );
    print_field("sha256", &summary.sha256);
}

fn print_current_database(session: &Session) {
    println!("seq  name  file");
    println!("{}", dim("---  ----  ----"));
    println!("0    main  {}", session.target().raw);
}

fn print_shell_show(state: &ShellState) {
    print_field("database", &state.session.target().raw);
    print_field(
        "circle",
        linked_circle(
            &state.session.target().network,
            &state.session.target().circle,
        ),
    );
    print_field("network", &state.session.target().network);
    print_field("rpc", state.session.rpc());
    print_field("wallet", state.session.caller());
    print_field("mode", state.mode.name());
    print_field("headers", if state.headers { "on" } else { "off" });
    print_field("timer", if state.timer { "on" } else { "off" });
    print_field(
        "output",
        state
            .output
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "stdout".to_string()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_and_save_args_are_sqlite_shaped() {
        assert_eq!(
            backup_path_from_args(&["backup.sqlite".to_string()]).unwrap(),
            PathBuf::from("backup.sqlite")
        );
        assert_eq!(
            backup_path_from_args(&["main".to_string(), "backup.sqlite".to_string()]).unwrap(),
            PathBuf::from("backup.sqlite")
        );
        assert!(backup_path_from_args(&["temp".to_string(), "backup.sqlite".to_string()]).is_err());
        assert!(backup_path_from_args(&[]).is_err());

        assert_eq!(
            save_path_from_args(&["copy.sqlite".to_string()]).unwrap(),
            PathBuf::from("copy.sqlite")
        );
        assert!(save_path_from_args(&["--append".to_string(), "copy.sqlite".to_string()]).is_err());
    }

    #[test]
    fn dot_parser_and_pipe_rejection_cover_redirection_shapes() {
        assert_eq!(
            parse_dot_parts(".once \"one shot.csv\"").unwrap(),
            vec![".once", "one shot.csv"]
        );
        assert_eq!(
            parse_dot_parts(".mode 'json'").unwrap(),
            vec![".mode", "json"]
        );
        assert!(parse_dot_parts(".output \"unterminated").is_err());
        assert!(reject_shell_pipe_arg("|cat", ".output").is_err());
        assert!(reject_shell_pipe_arg("file.sqlite", ".output").is_ok());
    }

    #[test]
    fn import_args_require_csv_and_reject_shell_pipes() {
        assert_eq!(
            import_args(&[
                "--csv".to_string(),
                "--skip".to_string(),
                "2".to_string(),
                "rows.csv".to_string(),
                "artist".to_string(),
            ])
            .unwrap(),
            (PathBuf::from("rows.csv"), "artist".to_string(), 2)
        );
        assert!(import_args(&["rows.csv".to_string(), "artist".to_string()]).is_err());
        assert!(import_args(&[
            "--csv".to_string(),
            "|cat".to_string(),
            "artist".to_string(),
        ])
        .is_err());
    }
}
