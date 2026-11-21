//! Session-start hook: hand the agent its Sillok status before it asks.
//!
//! Resuming used to cost every session a tool call, and the session label
//! that groups events was almost never set because nothing told the agent
//! its session id. The hook does both: it returns the repository's status
//! as session context, and where the host offers an environment file
//! (Claude Code's `CLAUDE_ENV_FILE`) it exports `SILLOK_SESSION` for every
//! later command.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::cli::args::read::StatusArgs;
use crate::commands::ctx::Ctx;
use crate::commands::status;
use crate::context::capture::SESSION_ENV;
use crate::error::SillokError;

/// File the host reads before each shell command; lines are `export` statements.
pub const ENV_FILE_ENV: &str = "CLAUDE_ENV_FILE";

/// Hosts cap added context (Claude Code at 10,000 characters); stay below it.
pub const MAX_CONTEXT_CHARS: usize = 8_000;

/// Recent records shown; fewer than `status` prints, since this is unasked.
const RECENT: usize = 5;

const LEAD: &str = "Sillok status for this repository, as `sillok status` prints it. Resume the open objective that fits the task or start one, and record work as it finishes; `sillok guide` prints the workflow.";

const EMPTY: &str = "Sillok has no open objectives or recent work for this repository. Start one with `sillok objective add` and record work as it finishes; `sillok guide` prints the workflow.";

/// Handles a session start and returns the hook's JSON output.
pub fn start(
    store: Option<PathBuf>,
    tz: Option<String>,
    session: &str,
    cwd: Option<&Path>,
) -> Result<String, SillokError> {
    // Status is scoped to the working directory, which the payload names.
    if let Some(dir) = cwd
        && dir.is_dir()
        && let Err(error) = std::env::set_current_dir(dir)
    {
        return Err(error.into());
    }
    if let Some(path) = std::env::var_os(ENV_FILE_ENV)
        && !path.is_empty()
        && let Err(error) = export_session(Path::new(&path), session)
    {
        return Err(error);
    }
    let mut ctx = match Ctx::new(store, tz, None, false) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let outcome = match status::status(
        &mut ctx,
        StatusArgs {
            all: false,
            limit: RECENT,
        },
    ) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut data = outcome.data;
    if !outcome.warnings.is_empty()
        && let Value::Object(map) = &mut data
    {
        map.insert("warnings".to_string(), json!(outcome.warnings));
    }
    Ok(json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": context(data),
        }
    })
    .to_string())
}

/// The text added to the session: a lead line and the status JSON, trimmed
/// oldest-first until it fits.
pub fn context(mut data: Value) -> String {
    let empty =
        ["objectives", "recent"]
            .iter()
            .all(|key| match data.get(*key).and_then(Value::as_array) {
                Some(list) => list.is_empty(),
                None => true,
            });
    if empty && data.get("warnings").is_none() {
        return EMPTY.to_string();
    }
    loop {
        let text = format!("{LEAD}\n{data}");
        if text.chars().count() <= MAX_CONTEXT_CHARS {
            return text;
        }
        // `recent` is newest first and `objectives` oldest first.
        let recent = match data.get_mut("recent").and_then(Value::as_array_mut) {
            Some(list) => list.pop().is_some(),
            None => false,
        };
        let trimmed = recent
            || match data.get_mut("objectives").and_then(Value::as_array_mut) {
                Some(list) if !list.is_empty() => {
                    list.remove(0);
                    true
                }
                Some(_) | None => false,
            };
        if !trimmed {
            return text.chars().take(MAX_CONTEXT_CHARS).collect();
        }
    }
}

/// Appends `export SILLOK_SESSION=...` once. The id was already checked
/// against a file-name alphabet, so it needs no quoting.
fn export_session(path: &Path, session: &str) -> Result<(), SillokError> {
    let line = format!("export {SESSION_ENV}={session}");
    match std::fs::read_to_string(path) {
        Ok(existing) if existing.lines().any(|known| known == line) => return Ok(()),
        Ok(_) | Err(_) => {}
    }
    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    match writeln!(file, "{line}") {
        Ok(()) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{EMPTY, MAX_CONTEXT_CHARS, context, export_session};

    #[test]
    fn empty_status_becomes_a_one_line_nudge() {
        assert_eq!(
            context(json!({"context": "/w", "objectives": [], "recent": []})),
            EMPTY
        );
    }

    #[test]
    fn long_status_is_trimmed_oldest_first() {
        let record = |name: &str| json!({"id": name, "text": "x".repeat(900)});
        let data = json!({
            "context": "/w",
            "objectives": (0..6).map(|n| record(&format!("o{n}"))).collect::<Vec<_>>(),
            "recent": (0..6).map(|n| record(&format!("r{n}"))).collect::<Vec<_>>(),
        });
        let text = context(data);
        assert!(text.chars().count() <= MAX_CONTEXT_CHARS);
        // The newest recent record and the newest objective survive longest.
        assert!(text.contains("\"r0\""));
        assert!(text.contains("\"o5\""));
        assert!(!text.contains("\"r5\""));
    }

    #[test]
    fn the_session_export_is_written_once() {
        let dir = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => panic!("temporary directory: {error}"),
        };
        let path = dir.path().join("env");
        for _ in 0..2 {
            if let Err(error) = export_session(&path, "abc-123") {
                panic!("export: {error}");
            }
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => assert_eq!(text, "export SILLOK_SESSION=abc-123\n"),
            Err(error) => panic!("read: {error}"),
        }
    }
}
