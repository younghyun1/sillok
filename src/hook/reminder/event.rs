//! The Claude Code and Codex hook payload fields Sillok reads.
//!
//! Both tools send one JSON object on stdin with `hook_event_name`, `session_id`,
//! `cwd`, and, for tool events, `tool_name` and `tool_input`. Everything else,
//! including tool output and the transcript path, is ignored.

use super::error::Error;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// PostToolUse carries tool output, so the bound is generous; larger input fails open.
pub const MAX_PAYLOAD: usize = 8 * 1024 * 1024;

/// Session ids name ledger files, so they are limited to a safe file-name alphabet.
const MAX_SESSION: usize = 128;

/// The events Sillok acts on; every other event is ignored.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// A tool call succeeded, in the main session or one of its subagents.
    ToolUse {
        session: String,
        cwd: Option<PathBuf>,
        tool: String,
        input: Value,
    },
    /// The main agent is ending its turn; `continued` is set once a Stop hook has
    /// already sent it back for another pass.
    Stop {
        session: String,
        continued: bool,
    },
    /// A main session started, resumed, or lost its context to compaction.
    SessionStart {
        session: String,
        cwd: Option<PathBuf>,
    },
    Ignored,
}

/// Parse one hook payload, rejecting malformed input instead of guessing.
pub fn parse(payload: &str) -> Result<Event, Error> {
    if payload.len() > MAX_PAYLOAD {
        return Err(Error::InputTooLarge);
    }
    let mut object = match serde_json::from_str::<Value>(payload) {
        Ok(Value::Object(object)) => object,
        Ok(_) => return Err(Error::Shape),
        Err(error) => return Err(Error::Json(error)),
    };
    let name = match object.get("hook_event_name") {
        Some(Value::String(name)) => name.clone(),
        _ => return Ok(Event::Ignored),
    };
    match name.as_str() {
        "PostToolUse" => tool_use(&mut object),
        "Stop" => {
            // Claude marks subagent payloads with agent_id; only the main turn owes a
            // record, and its ledger must survive until the main Stop reads it.
            if object.get("agent_id").is_some_and(|id| !id.is_null()) {
                return Ok(Event::Ignored);
            }
            let session = match session(&object) {
                Ok(session) => session,
                Err(error) => return Err(error),
            };
            let continued = matches!(object.get("stop_hook_active"), Some(Value::Bool(true)));
            Ok(Event::Stop { session, continued })
        }
        "SessionStart" => {
            if object.get("agent_id").is_some_and(|id| !id.is_null()) {
                return Ok(Event::Ignored);
            }
            match session(&object) {
                Ok(session) => Ok(Event::SessionStart {
                    session,
                    cwd: cwd(&object),
                }),
                Err(error) => Err(error),
            }
        }
        _ => Ok(Event::Ignored),
    }
}

fn cwd(object: &Map<String, Value>) -> Option<PathBuf> {
    match object.get("cwd") {
        Some(Value::String(cwd)) if !cwd.is_empty() => Some(PathBuf::from(cwd)),
        _ => None,
    }
}

/// Take the tool fields by value so large inputs are not copied.
fn tool_use(object: &mut Map<String, Value>) -> Result<Event, Error> {
    let session = match session(object) {
        Ok(session) => session,
        Err(error) => return Err(error),
    };
    let tool = match object.get("tool_name") {
        Some(Value::String(tool)) => tool.clone(),
        _ => return Err(Error::Shape),
    };
    let cwd = cwd(object);
    let input = match object.remove("tool_input") {
        Some(input) => input,
        None => Value::Null,
    };
    Ok(Event::ToolUse {
        session,
        cwd,
        tool,
        input,
    })
}

fn session(object: &Map<String, Value>) -> Result<String, Error> {
    match object.get("session_id") {
        Some(Value::String(id))
            if (1..=MAX_SESSION).contains(&id.len())
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') =>
        {
            Ok(id.clone())
        }
        _ => Err(Error::Session),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tool_and_stop_events() {
        let tool = parse(
            r#"{"hook_event_name":"PostToolUse","session_id":"abc-1","cwd":"/w","tool_name":"Bash","tool_input":{"command":"ls"}}"#,
        );
        assert!(matches!(
            tool,
            Ok(Event::ToolUse { ref session, ref tool, .. }) if session == "abc-1" && tool == "Bash"
        ));
        let stop =
            parse(r#"{"hook_event_name":"Stop","session_id":"abc_1","stop_hook_active":true}"#);
        assert!(matches!(
            stop,
            Ok(Event::Stop {
                continued: true,
                ..
            })
        ));
    }

    #[test]
    fn reads_session_starts() {
        let start = parse(
            r#"{"hook_event_name":"SessionStart","session_id":"s-1","cwd":"/w","source":"startup"}"#,
        );
        assert!(matches!(
            start,
            Ok(Event::SessionStart { ref session, cwd: Some(_) }) if session == "s-1"
        ));
        assert!(matches!(
            parse(r#"{"hook_event_name":"SessionStart","session_id":"s","agent_id":"child"}"#),
            Ok(Event::Ignored)
        ));
    }

    #[test]
    fn ignores_subagent_stops_and_other_events() {
        for payload in [
            r#"{"hook_event_name":"Stop","session_id":"a","agent_id":"child"}"#,
            r#"{"hook_event_name":"SubagentStop","session_id":"a"}"#,
            r#"{"hook_event_name":"Notification","session_id":"a"}"#,
            r#"{"type":"agent-turn-complete"}"#,
        ] {
            assert!(matches!(parse(payload), Ok(Event::Ignored)));
        }
    }

    #[test]
    fn rejects_unsafe_sessions_and_malformed_input() {
        for id in ["", "../escape", "a/b", "a b"] {
            let payload =
                serde_json::json!({"hook_event_name": "Stop", "session_id": id}).to_string();
            assert!(matches!(parse(&payload), Err(Error::Session)));
        }
        let long = "a".repeat(MAX_SESSION + 1);
        let payload =
            serde_json::json!({"hook_event_name": "Stop", "session_id": long}).to_string();
        assert!(matches!(parse(&payload), Err(Error::Session)));
        assert!(matches!(parse("[]"), Err(Error::Shape)));
        assert!(matches!(parse("{"), Err(Error::Json(_))));
        assert!(matches!(
            parse(r#"{"hook_event_name":"PostToolUse","session_id":"a"}"#),
            Err(Error::Shape)
        ));
        assert!(matches!(
            parse(&" ".repeat(MAX_PAYLOAD + 1)),
            Err(Error::InputTooLarge)
        ));
    }
}
