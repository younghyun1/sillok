//! Field extraction for tool inputs: shell text, patch targets, and MCP tool verbs.

use super::paths;
use serde_json::Value;
use std::path::Path;

/// MCP tool names whose first or second word is one of these write something.
const MCP_VERBS: &[&str] = &[
    "add",
    "apply",
    "approve",
    "archive",
    "assign",
    "batch",
    "cancel",
    "close",
    "complete",
    "convert",
    "create",
    "delete",
    "duplicate",
    "edit",
    "forward",
    "import",
    "invite",
    "label",
    "mark",
    "merge",
    "move",
    "post",
    "publish",
    "remove",
    "rename",
    "reopen",
    "reply",
    "resolve",
    "respond",
    "restore",
    "retire",
    "save",
    "schedule",
    "send",
    "set",
    "share",
    "spawn",
    "submit",
    "trash",
    "unarchive",
    "unlabel",
    "unmark",
    "unshare",
    "untrash",
    "update",
    "upload",
    "write",
];

/// Patch headers that name the file a patch writes.
const PATCH_HEADERS: &[&str] = &[
    "*** Add File: ",
    "*** Update File: ",
    "*** Delete File: ",
    "*** Move to: ",
];

/// Shell text from `tool_input.command`, which Codex may send as an argument vector.
pub fn command(input: &Value) -> Option<String> {
    match input.get("command") {
        Some(Value::String(script)) => Some(script.clone()),
        Some(Value::Array(words)) => {
            let mut script = String::new();
            for word in words {
                match word.as_str() {
                    Some(word) => {
                        if !script.is_empty() {
                            script.push(' ');
                        }
                        // Single-quote each word so the scanner sees the same argv.
                        script.push('\'');
                        script.push_str(&word.replace('\'', r"'\''"));
                        script.push('\'');
                    }
                    None => return None,
                }
            }
            Some(script)
        }
        Some(_) | None => None,
    }
}

/// The first non-scratch file a patch writes; a patch without headers still counts.
pub fn patch(input: &Value, cwd: Option<&Path>) -> Option<String> {
    let text = ["command", "input", "patch"]
        .iter()
        .find_map(|key| input.get(key).and_then(Value::as_str));
    let text = match text {
        Some(text) if !text.trim().is_empty() => text,
        Some(_) | None => return None,
    };
    let mut named = false;
    for line in text.lines() {
        let path = PATCH_HEADERS
            .iter()
            .find_map(|header| line.strip_prefix(header));
        if let Some(path) = path {
            named = true;
            if let Some(label) = paths::written(path.trim(), cwd) {
                return Some(format!("patch {label}"));
            }
        }
    }
    if named {
        None
    } else {
        Some("patch".to_owned())
    }
}

/// Browser `computer` actions that act on a page rather than look at it.
const BROWSER_INPUT: &[&str] = &[
    "double_click",
    "key",
    "left_click",
    "left_click_drag",
    "right_click",
    "triple_click",
    "type",
];

/// MCP tools named for a write. Browser tools count only when they click, type, or
/// fill a form; navigation, tabs, and page reads do not. Desktop-app session tools
/// (`ccd_*`) are interface actions, not work.
pub fn mcp(rest: &str, input: &Value) -> Option<String> {
    let (server, name) = match rest.rsplit_once("__") {
        Some(parts) => parts,
        None => return None,
    };
    if server.starts_with("ccd_") || name.starts_with("tabs_") {
        return None;
    }
    let browser_input = match name {
        "computer" => browser_writes("computer", input),
        "browser_batch" => match input.get("actions") {
            Some(Value::Array(actions)) => actions.iter().any(|action| {
                match (
                    action.get("name").and_then(Value::as_str),
                    action.get("input"),
                ) {
                    (Some(tool), Some(input)) => browser_writes(tool, input),
                    _ => false,
                }
            }),
            _ => false,
        },
        "form_input" | "file_upload" | "upload_image" => true,
        _ => return verb(name),
    };
    if browser_input {
        Some("browser input".to_owned())
    } else {
        None
    }
}

fn browser_writes(tool: &str, input: &Value) -> bool {
    match tool {
        "computer" => matches!(
            input.get("action").and_then(Value::as_str),
            Some(action) if BROWSER_INPUT.contains(&action)
        ),
        "form_input" | "file_upload" | "upload_image" => true,
        _ => false,
    }
}

fn verb(name: &str) -> Option<String> {
    let writes = name
        .split(['_', '-'])
        .filter(|word| !word.is_empty())
        .take(2)
        .any(|word| MCP_VERBS.contains(&word));
    if writes {
        Some(format!("mcp {name}"))
    } else {
        None
    }
}
