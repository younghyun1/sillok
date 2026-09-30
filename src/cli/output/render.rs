//! Printing results and errors.
//!
//! Data goes to stdout and nothing else does: errors, warnings, and logs go
//! to stderr, so `id=$(sillok note ...)` captures only the id.

use std::io::{ErrorKind, Write};

use serde_json::{Value, json};

use crate::cli::output::outcome::{Mode, Outcome, Shape};
use crate::domain::time::Timestamp;
use crate::error::SillokError;

/// Prints a successful result.
pub fn print_success(outcome: Outcome, mode: Mode) -> Result<(), SillokError> {
    let Outcome {
        command,
        shape,
        mut data,
        human,
        warnings,
    } = outcome;
    let stdout_text = match mode {
        Mode::Compact => match shape {
            Shape::Ids(ids) => {
                print_warnings(&warnings, mode);
                if ids.is_empty() {
                    None
                } else {
                    Some(ids.join("\n"))
                }
            }
            Shape::Data => {
                if !warnings.is_empty()
                    && let Value::Object(map) = &mut data
                {
                    map.insert("warnings".to_string(), json!(warnings));
                }
                match serde_json::to_string(&data) {
                    Ok(text) => Some(text),
                    Err(error) => return Err(error.into()),
                }
            }
            Shape::Text(text) => {
                print_warnings(&warnings, mode);
                Some(text)
            }
            Shape::Streamed => {
                print_warnings(&warnings, mode);
                None
            }
        },
        Mode::Json => {
            if matches!(shape, Shape::Streamed) {
                print_warnings(&warnings, mode);
                None
            } else {
                let envelope = json!({
                    "ok": true,
                    "command": command,
                    "generated_at": Timestamp::now(),
                    "data": data,
                    "warnings": warnings,
                });
                match serde_json::to_string(&envelope) {
                    Ok(text) => Some(text),
                    Err(error) => return Err(error.into()),
                }
            }
        }
        Mode::Human => {
            print_warnings(&warnings, mode);
            match (human, shape) {
                (Some(text), _) => Some(text),
                (None, Shape::Text(text)) => Some(text),
                (None, Shape::Streamed) => None,
                (None, _) => match serde_json::to_string_pretty(&data) {
                    Ok(text) => Some(text),
                    Err(error) => return Err(error.into()),
                },
            }
        }
    };
    match stdout_text {
        Some(text) => write_stdout(&text),
        None => Ok(()),
    }
}

/// Prints a failure to stderr.
pub fn print_failure(command: &'static str, error: &SillokError, mode: Mode) {
    let text = match mode {
        Mode::Compact => json!({ "error": error.code(), "message": error.to_string() }).to_string(),
        Mode::Json => json!({
            "ok": false,
            "command": command,
            "generated_at": Timestamp::now(),
            "error": { "code": error.code(), "message": error.to_string() },
        })
        .to_string(),
        Mode::Human => format!("error ({}): {error}", error.code()),
    };
    let mut stderr = std::io::stderr().lock();
    match writeln!(stderr, "{text}") {
        Ok(()) | Err(_) => {}
    }
}

fn print_warnings(warnings: &[String], mode: Mode) {
    if warnings.is_empty() {
        return;
    }
    let text = match mode {
        Mode::Human => warnings
            .iter()
            .map(|warning| format!("warning: {warning}"))
            .collect::<Vec<_>>()
            .join("\n"),
        Mode::Compact | Mode::Json => json!({ "warnings": warnings }).to_string(),
    };
    let mut stderr = std::io::stderr().lock();
    match writeln!(stderr, "{text}") {
        Ok(()) | Err(_) => {}
    }
}

/// Writes one block to stdout. A closed pipe (`sillok export | head`) is
/// not an error: the reader got what it wanted.
pub fn write_stdout(text: &str) -> Result<(), SillokError> {
    let mut stdout = std::io::stdout().lock();
    match writeln!(stdout, "{text}") {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error.into()),
    }
}
