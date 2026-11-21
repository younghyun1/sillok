//! `skill` commands: install targets, idempotency, replacement, and duplicates.

mod support;

use std::path::Path;

use support::{Sandbox, TestResult, fail, strings};

fn write(path: &Path, text: &str) -> TestResult {
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return Err(Box::new(error));
    }
    match std::fs::write(path, text) {
        Ok(()) => Ok(()),
        Err(error) => Err(Box::new(error)),
    }
}

fn read(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    match std::fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(error) => Err(fail(format!("read {}: {error}", path.display()))),
    }
}

#[test]
fn install_writes_one_copy_per_tool_and_is_idempotent() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let claude = sandbox.path(".claude/skills/sillok/SKILL.md");
    let codex = sandbox.path(".codex/skills/sillok/SKILL.md");
    // Claude Code has a hand-written copy; Codex is set up but has none.
    if let Err(error) = write(&claude, "---\nname: sillok\n---\n\nOld notes.\n") {
        return Err(error);
    }
    if let Err(error) = std::fs::create_dir_all(sandbox.path(".codex")) {
        return Err(Box::new(error));
    }
    let (first, second, printed) = match (
        sandbox.json(&["skill", "install"]),
        sandbox.json(&["skill", "install"]),
        sandbox.ok(&["skill", "print"]),
    ) {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => return Err(error),
    };
    assert_eq!(strings(&first, "/targets", "/tool"), ["claude", "codex"]);
    assert_eq!(
        strings(&first, "/targets", "/action"),
        ["updated", "installed"]
    );
    assert_eq!(
        strings(&second, "/targets", "/action"),
        ["unchanged", "unchanged"]
    );
    let (claude_text, codex_text) = match (read(&claude), read(&codex)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    assert_eq!(claude_text, format!("{printed}\n"));
    assert_eq!(codex_text, claude_text);
    assert!(claude_text.contains(concat!(
        "sillok-version: \"",
        env!("CARGO_PKG_VERSION"),
        "\""
    )));
    // The body is exactly the guide.
    let guide = match sandbox.ok(&["guide"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(printed.ends_with(&guide));
    // No second root was created for Codex, and nothing but the skill was written.
    assert!(!sandbox.path(".agents").exists());
    let state = match sandbox.json(&["skill", "status"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(
        strings(&state, "/targets", "/state"),
        ["current", "current"]
    );
    Ok(())
}

#[test]
fn codex_keeps_its_existing_root_and_reports_a_duplicate() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let own = sandbox.path(".codex/skills/sillok/SKILL.md");
    let shared = sandbox.path(".agents/skills/sillok/SKILL.md");
    if let Err(error) = std::fs::create_dir_all(sandbox.path(".codex")) {
        return Err(Box::new(error));
    }
    if let Err(error) = write(&shared, "# Shared copy\n") {
        return Err(error);
    }
    // The only copy lives in the shared root, so that one is replaced.
    let first = match sandbox.json(&["skill", "install", "--tool", "codex"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(strings(&first, "/targets", "/action"), ["updated"]);
    assert!(!own.exists());
    // With a copy in both roots, the tool's own is kept current and the other is named.
    if let Err(error) = write(&own, "# Own copy\n") {
        return Err(error);
    }
    let second = match sandbox.json(&["skill", "install", "--tool", "codex"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(
        second["targets"][0]["path"].as_str(),
        Some(own.display().to_string().as_str())
    );
    let warning = match second["warnings"][0].as_str() {
        Some(value) => value.to_string(),
        None => return Err(fail("expected a duplicate warning")),
    };
    assert!(warning.contains(".agents/skills/sillok/SKILL.md"));
    Ok(())
}

#[test]
fn explicit_roots_and_missing_tools() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    // No tool is set up in this home.
    let (code, error) = match sandbox.error(&["skill", "install"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(code, 1);
    assert_eq!(error["error"], "skill_no_target");
    let root = sandbox.path("project/.claude/skills");
    let root_text = root.display().to_string();
    let installed =
        match sandbox.json(&["skill", "install", "--dir", &root_text, "--dir", &root_text]) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
    assert_eq!(strings(&installed, "/targets", "/action"), ["installed"]);
    assert!(root.join("sillok/SKILL.md").is_file());
    // An explicit root does not also install for tools.
    assert!(!sandbox.path(".claude").exists());
    Ok(())
}

#[test]
fn status_leaves_foreign_and_current_skills_alone() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let claude = sandbox.path(".claude/skills/sillok/SKILL.md");
    if let Err(error) = write(&claude, "# Hand written\n") {
        return Err(error);
    }
    let status = match sandbox.json(&["status"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(status.get("warnings").is_none());
    match read(&claude) {
        Ok(text) => assert_eq!(text, "# Hand written\n"),
        Err(error) => return Err(error),
    }
    let state = match sandbox.json(&["skill", "status"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(strings(&state, "/targets", "/state"), ["unmanaged"]);
    Ok(())
}

#[test]
fn status_refreshes_a_copy_left_by_an_older_release() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let claude = sandbox.path(".claude/skills/sillok/SKILL.md");
    if let Err(error) = write(&claude, &sillok::skill::content::render_as("0.0.1")) {
        return Err(error);
    }
    let (first, second) = match (sandbox.json(&["status"]), sandbox.json(&["status"])) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    let notice = match first["warnings"][0].as_str() {
        Some(value) => value.to_string(),
        None => return Err(fail("expected a refresh notice")),
    };
    assert!(notice.contains("refreshed the sillok skill"));
    assert!(second.get("warnings").is_none());
    match read(&claude) {
        Ok(text) => assert_eq!(text, sillok::skill::content::render()),
        Err(error) => return Err(error),
    }
    Ok(())
}
