//! `sillok hook` through the protocol Claude Code and Codex share, and its
//! registration by `skill install --hooks`.

mod support;

use std::process::Output;

use serde_json::{Value, json};
use support::{Sandbox, TestResult, fail, run_input, strings};

/// Sends one hook payload, with the reminder ledger kept in the sandbox.
fn hook(
    sandbox: &Sandbox,
    payload: &Value,
    envs: &[(&str, &str)],
) -> Result<Output, Box<dyn std::error::Error>> {
    let ledger = sandbox.path("ledger").display().to_string();
    let mut all = vec![("SILLOK_REMINDER_DIR", ledger.as_str())];
    all.extend_from_slice(envs);
    run_input(
        &sandbox.store,
        sandbox.dir.path(),
        &["hook"],
        &all,
        Some(&payload.to_string()),
    )
}

fn tool(session: &str, command: &str) -> Value {
    json!({
        "hook_event_name": "PostToolUse",
        "session_id": session,
        "cwd": "/work/repo",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "tool_response": {"stdout": "ignored"},
    })
}

fn stop(session: &str, continued: bool) -> Value {
    json!({"hook_event_name": "Stop", "session_id": session, "stop_hook_active": continued})
}

#[test]
fn session_start_returns_status_and_exports_the_session() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let start = json!({
        "hook_event_name": "SessionStart",
        "session_id": "sess-42",
        "cwd": sandbox.dir.path().display().to_string(),
        "source": "startup",
    });
    let env_file = sandbox.path("env").display().to_string();
    // Nothing recorded yet: a one-line nudge, and the session is exported.
    let empty = match hook(&sandbox, &start, &[("CLAUDE_ENV_FILE", env_file.as_str())]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(empty.status.success());
    let first: Value = match serde_json::from_slice(&empty.stdout) {
        Ok(value) => value,
        Err(error) => return Err(fail(format!("bad hook output: {error}"))),
    };
    assert_eq!(first["hookSpecificOutput"]["hookEventName"], "SessionStart");
    let nudge = match first["hookSpecificOutput"]["additionalContext"].as_str() {
        Some(value) => value.to_string(),
        None => return Err(fail("no context")),
    };
    assert!(nudge.starts_with("Sillok has no open objectives"));
    match std::fs::read_to_string(sandbox.path("env")) {
        Ok(text) => assert_eq!(text, "export SILLOK_SESSION=sess-42\n"),
        Err(error) => return Err(Box::new(error)),
    }
    // With work recorded, the context carries the status payload.
    let objective = match sandbox.ok(&["objective", "add", "Resume me"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let again = match hook(&sandbox, &start, &[]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let second: Value = match serde_json::from_slice(&again.stdout) {
        Ok(value) => value,
        Err(error) => return Err(fail(format!("bad hook output: {error}"))),
    };
    let context = match second["hookSpecificOutput"]["additionalContext"].as_str() {
        Some(value) => value.to_string(),
        None => return Err(fail("no context")),
    };
    assert!(context.contains(&objective));
    assert!(context.contains("Resume me"));
    Ok(())
}

#[test]
fn an_unrecorded_change_sends_the_turn_back_once() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let outputs = match (
        hook(&sandbox, &tool("t1", "git push"), &[]),
        hook(&sandbox, &stop("t1", false), &[]),
        // The continuation records the work, and its Stop ends the turn.
        hook(&sandbox, &tool("t1", "sillok note \"pushed\""), &[]),
        hook(&sandbox, &stop("t1", true), &[]),
        // A turn that records what it changed is never sent back.
        hook(&sandbox, &tool("t2", "git push && sillok note done"), &[]),
        hook(&sandbox, &stop("t2", false), &[]),
    ) {
        (Ok(a), Ok(b), Ok(c), Ok(d), Ok(e), Ok(f)) => [a, b, c, d, e, f],
        _ => return Err(fail("a hook invocation failed to run")),
    };
    for output in &outputs {
        assert!(output.status.success());
    }
    let blocked: Value = match serde_json::from_slice(&outputs[1].stdout) {
        Ok(value) => value,
        Err(error) => return Err(fail(format!("bad stop output: {error}"))),
    };
    assert_eq!(blocked["decision"], "block");
    assert!(
        blocked["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("git push"))
    );
    for quiet in [0, 2, 3, 4, 5] {
        assert!(
            outputs[quiet].stdout.is_empty(),
            "invocation {quiet} printed output"
        );
    }
    Ok(())
}

#[test]
fn malformed_input_never_fails_or_prints() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    for payload in [
        "",
        "{",
        "[]",
        "{\"hook_event_name\":\"Stop\",\"session_id\":\"../x\"}",
    ] {
        let output = match run_input(
            &sandbox.store,
            sandbox.dir.path(),
            &["hook"],
            &[],
            Some(payload),
        ) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        assert!(
            output.status.success(),
            "payload `{payload}` failed the hook"
        );
        assert!(output.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn skill_install_registers_hooks_once() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    for dir in [".claude", ".codex"] {
        if let Err(error) = std::fs::create_dir_all(sandbox.path(dir)) {
            return Err(Box::new(error));
        }
    }
    let settings = sandbox.path(".claude/settings.json");
    if let Err(error) = std::fs::write(&settings, "{\"theme\":\"dark\"}") {
        return Err(Box::new(error));
    }
    // Without the flag, no configuration is touched.
    if let Err(error) = sandbox.ok(&["skill", "install"]) {
        return Err(error);
    }
    assert!(!sandbox.path(".codex/hooks.json").exists());
    let (first, second, state) = match (
        sandbox.json(&["skill", "install", "--hooks"]),
        sandbox.json(&["skill", "install", "--hooks"]),
        sandbox.json(&["skill", "status"]),
    ) {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => return Err(error),
    };
    assert_eq!(
        strings(&first, "/targets", "/hooks/action"),
        ["installed", "installed"]
    );
    assert_eq!(
        strings(&second, "/targets", "/hooks/action"),
        ["unchanged", "unchanged"]
    );
    assert_eq!(state["targets"][0]["hooks"], true);
    assert_eq!(state["targets"][1]["hooks"], true);
    // Codex needs its new hooks trusted, and says so once.
    assert!(
        first["warnings"][0]
            .as_str()
            .is_some_and(|text| text.contains("/hooks"))
    );
    assert!(second.get("warnings").is_none());
    let written: Value = match std::fs::read(&settings).map(|bytes| serde_json::from_slice(&bytes))
    {
        Ok(Ok(value)) => value,
        _ => return Err(fail("settings.json is not valid JSON")),
    };
    assert_eq!(written["theme"], "dark");
    for event in ["SessionStart", "PostToolUse", "Stop"] {
        assert_eq!(written["hooks"][event].as_array().map(Vec::len), Some(1));
    }
    Ok(())
}
