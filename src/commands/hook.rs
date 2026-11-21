//! `sillok hook`: one entry point for the Claude Code and Codex hook events.
//!
//! The host pipes one JSON object to stdin per event. `SessionStart` returns
//! the repository's status as context; `PostToolUse` and `Stop` drive the
//! end-of-turn reminder. A hook must never break a session, so every failure
//! is logged to stderr and the command still exits successfully with no
//! output. Output goes straight to stdout: the host parses it, so the usual
//! output modes do not apply.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::cli::output::outcome::Outcome;
use crate::hook::reminder::classify;
use crate::hook::reminder::error::Error;
use crate::hook::reminder::event::{self, Event};
use crate::hook::reminder::ledger;
use crate::hook::session;
use crate::logging;

/// How long the host gets to close stdin.
const STDIN_DEADLINE: Duration = Duration::from_secs(2);

/// `hook`.
pub fn run(store: Option<PathBuf>, tz: Option<String>) -> Outcome {
    match handle(store, tz) {
        Ok(Some(output)) => {
            let mut stdout = std::io::stdout().lock();
            if let Err(error) = writeln!(stdout, "{output}") {
                logging::ensure();
                tracing::warn!(error = %error, "sillok hook output failed");
            }
        }
        Ok(None) => {}
        Err(message) => {
            logging::ensure();
            tracing::warn!(error = %message, "sillok hook skipped");
        }
    }
    Outcome::streamed("hook", Value::Null)
}

/// Handles one event; the result is the JSON to print, if any.
fn handle(store: Option<PathBuf>, tz: Option<String>) -> Result<Option<String>, String> {
    let payload = match stdin_payload() {
        Ok(value) => value,
        Err(error) => return Err(error.to_string()),
    };
    let directory = ledger::directory();
    match event::parse(&payload) {
        Ok(Event::ToolUse {
            session,
            cwd,
            tool,
            input,
        }) => {
            let activity = classify::classify(&tool, &input, cwd.as_deref());
            match ledger::note(&directory, &session, &activity) {
                Ok(()) => Ok(None),
                Err(error) => Err(error.to_string()),
            }
        }
        Ok(Event::Stop { session, continued }) => {
            // Always consume the ledger so the next turn starts empty.
            let turn = match ledger::take(&directory, &session) {
                Ok(value) => value,
                Err(error) => return Err(error.to_string()),
            };
            if continued {
                return Ok(None);
            }
            Ok(turn
                .reminder()
                .map(|reason| json!({ "decision": "block", "reason": reason }).to_string()))
        }
        Ok(Event::SessionStart { session, cwd }) => {
            match session::start(store, tz, &session, cwd.as_deref()) {
                Ok(output) => Ok(Some(output)),
                Err(error) => Err(error.to_string()),
            }
        }
        Ok(Event::Ignored) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Reads bounded hook input without waiting indefinitely for stdin to close.
fn stdin_payload() -> Result<String, Error> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut text = String::new();
        let result = std::io::stdin()
            .take((event::MAX_PAYLOAD + 1) as u64)
            .read_to_string(&mut text)
            .map(|_| text);
        // The receiver is gone only after its deadline, when the result is moot.
        match sender.send(result) {
            Ok(()) | Err(_) => {}
        }
    });
    match receiver.recv_timeout(STDIN_DEADLINE) {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(error)) => Err(Error::Io(error)),
        Err(_) => Err(Error::InputTimeout),
    }
}
