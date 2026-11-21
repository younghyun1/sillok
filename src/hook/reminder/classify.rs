//! Decide whether one tool call changed files or systems, or wrote to Sillok.
//!
//! Tool names cover both hosts: Claude Code's `Bash`, `Edit`, `Write`, `MultiEdit`,
//! `NotebookEdit`, `Artifact`, and `ArtifactData`, Codex's `apply_patch`, and MCP
//! tools of either, named `mcp__<server>__<tool>`.

use super::{paths, shell::scan, tool_input};
use serde_json::Value;
use std::path::Path;

/// What one tool call did, as far as the reminder is concerned.
#[derive(Debug, Default, PartialEq)]
pub struct Activity {
    /// A short description of the first change, without argument values.
    pub change: Option<String>,
    /// The call wrote a Sillok record.
    pub record: bool,
}

/// Classify one successful tool call run from `cwd`.
pub fn classify(tool: &str, input: &Value, cwd: Option<&Path>) -> Activity {
    match tool {
        "Bash" | "shell" | "local_shell" | "exec_command" => match tool_input::command(input) {
            Some(script) => {
                let result = scan::scan(&script, cwd);
                Activity {
                    change: result.change,
                    record: result.record,
                }
            }
            None => Activity::default(),
        },
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            let target = ["file_path", "notebook_path", "path"]
                .iter()
                .find_map(|key| input.get(key).and_then(Value::as_str));
            match target {
                Some(path) => {
                    changed(paths::written(path, cwd).map(|label| format!("edit {label}")))
                }
                // Codex reports some apply_patch calls under these names, with the patch text.
                None => changed(tool_input::patch(input, cwd)),
            }
        }
        "apply_patch" => changed(tool_input::patch(input, cwd)),
        "Artifact" => match input.get("action").and_then(Value::as_str) {
            None | Some("publish") => changed(Some("artifact publish".to_owned())),
            Some(action @ ("delete" | "pin" | "unpin")) => {
                changed(Some(format!("artifact {action}")))
            }
            Some(_) => Activity::default(),
        },
        "ArtifactData" => match input.get("action").and_then(Value::as_str) {
            Some(action @ ("set" | "update" | "delete" | "batch")) => {
                changed(Some(format!("artifact data {action}")))
            }
            Some(_) | None => Activity::default(),
        },
        _ => match tool.strip_prefix("mcp__") {
            Some(rest) => changed(tool_input::mcp(rest, input)),
            None => Activity::default(),
        },
    }
}

fn changed(change: Option<String>) -> Activity {
    Activity {
        change,
        record: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CWD: &str = "/work/repo";

    fn change(tool: &str, input: Value) -> Option<String> {
        classify(tool, &input, Some(Path::new(CWD))).change
    }

    #[test]
    fn edits_count_outside_scratch() {
        assert_eq!(
            change("Edit", json!({"file_path": "/work/repo/plan.md"})).as_deref(),
            Some("edit plan.md")
        );
        let scratch = std::env::temp_dir().join("note.md");
        assert_eq!(
            change("Write", json!({"file_path": scratch.to_string_lossy()})),
            None
        );
        assert_eq!(
            change("NotebookEdit", json!({"notebook_path": "a.ipynb"})).as_deref(),
            Some("edit a.ipynb")
        );
    }

    #[test]
    fn patches_name_their_files() {
        let patch = "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-a\n+b\n*** End Patch";
        assert_eq!(
            change("apply_patch", json!({"command": patch})).as_deref(),
            Some("patch lib.rs")
        );
        assert_eq!(
            change("apply_patch", json!({"command": "raw diff"})).as_deref(),
            Some("patch")
        );
        assert_eq!(change("apply_patch", json!({})), None);
    }

    #[test]
    fn shell_commands_carry_changes_and_records() {
        let activity = classify(
            "Bash",
            &json!({"command": "git commit -m x && sillok note done"}),
            Some(Path::new(CWD)),
        );
        assert_eq!(
            activity,
            Activity {
                change: Some("git commit".into()),
                record: true
            }
        );
        let argv = json!({"command": ["bash", "-lc", "gh pr merge 1 --squash"]});
        assert_eq!(change("Bash", argv).as_deref(), Some("gh pr merge"));
        assert_eq!(change("Bash", json!({"command": 3})), None);
    }

    #[test]
    fn mcp_writes_by_verb_and_skips_interface_tools() {
        assert_eq!(
            change("mcp__plugin_linear_linear__save_issue", json!({})).as_deref(),
            Some("mcp save_issue")
        );
        assert_eq!(
            change("mcp__ae77__slack_send_message_draft", json!({})).as_deref(),
            Some("mcp slack_send_message_draft")
        );
        assert_eq!(
            change("mcp__notion__notion-update-page", json!({})).as_deref(),
            Some("mcp notion-update-page")
        );
        for tool in [
            "mcp__plugin_linear_linear__get_issue",
            "mcp__ae77__slack_read_channel",
            "mcp__ccd_session__mark_chapter",
            "mcp__malformed",
            "Read",
            "Agent",
        ] {
            assert_eq!(change(tool, json!({})), None, "{tool}");
        }
    }

    #[test]
    fn browser_tools_count_only_page_input() {
        let browser = "mcp__Claude_Browser__";
        for (name, input) in [
            ("navigate", json!({"url": "https://example.com"})),
            ("tabs_create", json!({})),
            ("computer", json!({"action": "screenshot"})),
            (
                "browser_batch",
                json!({"actions": [{"name": "navigate", "input": {"url": "x"}}, {"name": "computer", "input": {"action": "screenshot"}}]}),
            ),
        ] {
            assert_eq!(change(&format!("{browser}{name}"), input), None, "{name}");
        }
        for (name, input) in [
            ("computer", json!({"action": "left_click", "ref": "ref_1"})),
            ("form_input", json!({"ref": "ref_2", "value": "x"})),
            (
                "browser_batch",
                json!({"actions": [{"name": "navigate", "input": {}}, {"name": "computer", "input": {"action": "type", "text": "x"}}]}),
            ),
        ] {
            assert_eq!(
                change(&format!("mcp__claude-in-chrome__{name}"), input).as_deref(),
                Some("browser input"),
                "{name}"
            );
        }
    }

    #[test]
    fn artifacts_count_when_published_or_written() {
        assert_eq!(
            change("Artifact", json!({"file_path": "a.html"})).as_deref(),
            Some("artifact publish")
        );
        assert_eq!(change("Artifact", json!({"action": "read"})), None);
        assert_eq!(
            change("ArtifactData", json!({"action": "batch"})).as_deref(),
            Some("artifact data batch")
        );
        assert_eq!(change("ArtifactData", json!({"action": "query"})), None);
    }
}
