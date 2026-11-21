//! Registering `sillok hook` with Claude Code and Codex.
//!
//! Both tools read a JSON file whose `hooks` object maps an event to groups
//! of handlers. Registration converges that file on exactly one Sillok
//! handler per event: entries Sillok wrote earlier, and entries for the
//! standalone `sillok-reminder` binary this command replaces, are removed
//! first, so running it twice changes nothing and nothing fires twice.
//! Every other key and handler is left as it was.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::error::SillokError;
use crate::skill::file;
use crate::skill::roots::Tool;

/// Handler command. The guard makes it a no-op on a machine that shares the
/// configuration but has no `sillok` on `PATH`.
pub const COMMAND: &str = "command -v sillok >/dev/null 2>&1 || exit 0; exec sillok hook";

/// Substrings that mark a handler as Sillok's: its own command, and the
/// standalone reminder it supersedes.
const OWNED: [&str; 2] = ["sillok hook", "sillok-reminder"];

/// What registration did to one configuration file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The file had no Sillok handlers.
    Installed,
    /// Existing Sillok or `sillok-reminder` handlers were replaced.
    Updated,
    /// The file already held exactly these handlers.
    Unchanged,
}

impl Action {
    /// Stable label used in output.
    pub fn label(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
        }
    }
}

/// Events Sillok handles, with the tool-call matcher and timeout in seconds.
fn events(tool: Tool) -> [(&'static str, Option<&'static str>, u32); 3] {
    let tools = match tool {
        Tool::Claude => "Bash|Edit|Write|MultiEdit|NotebookEdit|Artifact|ArtifactData|mcp__.*",
        Tool::Codex => "Bash|apply_patch|Edit|Write|mcp__.*",
    };
    [
        ("SessionStart", None, 10),
        ("PostToolUse", Some(tools), 5),
        // Synchronous on purpose: an async Stop hook cannot continue the turn.
        ("Stop", None, 5),
    ]
}

/// Whether a handler object belongs to Sillok.
fn owned(handler: &Value) -> bool {
    let command = match handler.get("command").and_then(Value::as_str) {
        Some(value) => value,
        None => return false,
    };
    OWNED.iter().any(|marker| command.contains(marker))
}

/// Rewrites `root` in place and reports how many owned handlers it removed.
fn apply(root: &mut Map<String, Value>, tool: Tool) -> Result<usize, SillokError> {
    let hooks = match root
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
    {
        Value::Object(map) => map,
        _ => return Err(shape("`hooks` is not an object")),
    };
    let mut removed = 0;
    for (event, matcher, timeout) in events(tool) {
        let groups = match hooks
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
        {
            Value::Array(list) => list,
            _ => return Err(shape("a hook event is not an array")),
        };
        for group in groups.iter_mut() {
            if let Some(Value::Array(handlers)) = group.get_mut("hooks") {
                let before = handlers.len();
                handlers.retain(|handler| !owned(handler));
                removed += before - handlers.len();
            }
        }
        // A group emptied by the removal would be a matcher with nothing to run.
        groups.retain(|group| match group.get("hooks") {
            Some(Value::Array(handlers)) => !handlers.is_empty(),
            Some(_) | None => true,
        });
        let mut group = Map::new();
        if let Some(pattern) = matcher {
            group.insert("matcher".to_string(), json!(pattern));
        }
        group.insert(
            "hooks".to_string(),
            json!([{ "type": "command", "command": COMMAND, "timeout": timeout }]),
        );
        groups.push(Value::Object(group));
    }
    Ok(removed)
}

/// Makes `path` register Sillok's hooks for `tool`.
pub fn register(path: &Path, tool: Tool) -> Result<Action, SillokError> {
    let original = match read(path) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut updated = original.clone();
    let removed = match apply(&mut updated, tool) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if updated == original {
        return Ok(Action::Unchanged);
    }
    let mut text = match serde_json::to_string_pretty(&Value::Object(updated)) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    text.push('\n');
    match file::write_atomic(path, &text) {
        Ok(()) => Ok(match removed {
            0 => Action::Installed,
            _ => Action::Updated,
        }),
        Err(error) => Err(error),
    }
}

/// Whether `path` already registers exactly Sillok's hooks.
pub fn registered(path: &Path, tool: Tool) -> Result<bool, SillokError> {
    let original = match read(path) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut updated = original.clone();
    match apply(&mut updated, tool) {
        Ok(_) => Ok(updated == original),
        Err(error) => Err(error),
    }
}

/// Reads the configuration object; a missing or empty file is an empty one.
fn read(path: &Path) -> Result<Map<String, Value>, SillokError> {
    let bytes = match std::fs::read(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(error.into()),
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(shape("the file is not a JSON object")),
        Err(error) => Err(shape(&format!(
            "{} is not valid JSON ({error}); fix it and run this again",
            path.display()
        ))),
    }
}

fn shape(message: &str) -> SillokError {
    SillokError::invalid("hook_config_error", message.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{Action, COMMAND, register, registered};
    use crate::skill::roots::Tool;

    fn load(path: &std::path::Path) -> Value {
        match std::fs::read(path).map(|bytes| serde_json::from_slice::<Value>(&bytes)) {
            Ok(Ok(value)) => value,
            _ => panic!("unreadable {}", path.display()),
        }
    }

    fn commands(value: &Value, event: &str) -> Vec<String> {
        let mut found = Vec::new();
        if let Some(groups) = value["hooks"][event].as_array() {
            for group in groups {
                if let Some(handlers) = group["hooks"].as_array() {
                    for handler in handlers {
                        if let Some(command) = handler["command"].as_str() {
                            found.push(command.to_string());
                        }
                    }
                }
            }
        }
        found
    }

    #[test]
    fn registration_is_idempotent_and_keeps_other_settings() {
        let dir = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => panic!("temporary directory: {error}"),
        };
        let path = dir.path().join("settings.json");
        let existing = json!({
            "model": "kept",
            "hooks": {
                "Stop": [
                    {"hooks": [{"type": "command", "command": "/usr/local/bin/alert stop", "async": true}]},
                    {"hooks": [{"type": "command", "command": "test -x \"$HOME/.local/bin/sillok-reminder\" || exit 0; exec \"$HOME/.local/bin/sillok-reminder\"", "timeout": 5}]}
                ],
                "PostToolUse": [
                    {"matcher": "Write|Edit", "hooks": [
                        {"type": "command", "command": "/hooks/guard.sh"},
                        {"type": "command", "command": "exec \"$HOME/.local/bin/sillok-reminder\""}
                    ]}
                ],
                "Notification": [{"hooks": [{"type": "command", "command": "/usr/local/bin/alert"}]}]
            }
        });
        if let Err(error) = std::fs::write(&path, existing.to_string()) {
            panic!("write: {error}");
        }
        assert!(matches!(registered(&path, Tool::Claude), Ok(false)));
        assert!(matches!(register(&path, Tool::Claude), Ok(Action::Updated)));
        let first = load(&path);
        assert!(matches!(
            register(&path, Tool::Claude),
            Ok(Action::Unchanged)
        ));
        assert_eq!(load(&path), first);
        assert!(matches!(registered(&path, Tool::Claude), Ok(true)));
        // One Sillok handler per event, the old reminder gone, the rest intact.
        assert_eq!(first["model"], "kept");
        assert_eq!(commands(&first, "SessionStart"), [COMMAND]);
        assert_eq!(
            commands(&first, "Stop"),
            ["/usr/local/bin/alert stop", COMMAND]
        );
        assert_eq!(
            commands(&first, "PostToolUse"),
            ["/hooks/guard.sh", COMMAND]
        );
        assert_eq!(commands(&first, "Notification"), ["/usr/local/bin/alert"]);
        assert_eq!(first["hooks"]["PostToolUse"][0]["matcher"], "Write|Edit");
        assert!(first["hooks"]["Stop"][1].get("matcher").is_none());
    }

    #[test]
    fn a_missing_file_is_created_and_a_broken_one_is_refused() {
        let dir = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => panic!("temporary directory: {error}"),
        };
        let fresh = dir.path().join("codex").join("hooks.json");
        assert!(matches!(
            register(&fresh, Tool::Codex),
            Ok(Action::Installed)
        ));
        let value = load(&fresh);
        assert_eq!(commands(&value, "Stop"), [COMMAND]);
        assert!(
            value["hooks"]["PostToolUse"][0]["matcher"]
                .as_str()
                .is_some_and(|matcher| matcher.contains("apply_patch"))
        );
        let broken = dir.path().join("broken.json");
        if let Err(error) = std::fs::write(&broken, "{ not json") {
            panic!("write: {error}");
        }
        assert!(matches!(
            register(&broken, Tool::Claude),
            Err(error) if error.code() == "hook_config_error"
        ));
        match std::fs::read_to_string(&broken) {
            Ok(text) => assert_eq!(text, "{ not json"),
            Err(error) => panic!("read: {error}"),
        }
    }
}
