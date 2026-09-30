//! Two replicas syncing through a local bare repository.

mod support;

use std::path::Path;

use support::{Sandbox, TestResult, fail, git, strings};

fn bare_remote(sandbox: &Sandbox) -> Result<String, Box<dyn std::error::Error>> {
    let remote = sandbox.path("remote.git");
    match git(
        sandbox.dir.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            &remote.display().to_string(),
        ],
    ) {
        Ok(_) => Ok(remote.display().to_string()),
        Err(error) => Err(error),
    }
}

fn replica(remote: &str) -> Result<Sandbox, Box<dyn std::error::Error>> {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    match sandbox.ok(&["sync", "remote", "set", remote]) {
        Ok(_) => Ok(sandbox),
        Err(error) => Err(error),
    }
}

fn remote_file(remote: &str, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    git(Path::new(remote), &["show", &format!("main:{path}")])
}

#[test]
fn replicas_converge_and_noop_syncs_do_not_commit() -> TestResult {
    let host = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let remote = match bare_remote(&host) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let (a, b) = match (replica(&remote), replica(&remote)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    for (side, text) in [(&a, "from a 1"), (&a, "from a 2"), (&b, "from b")] {
        if let Err(error) = side.ok(&["note", text]) {
            return Err(error);
        }
    }
    let (first, second, third) = match (a.json(&["sync"]), b.json(&["sync"]), a.json(&["sync"])) {
        (Ok(x), Ok(y), Ok(z)) => (x, y, z),
        (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => return Err(error),
    };
    assert_eq!(
        (first["pulled"].as_u64(), first["pushed"].as_u64()),
        (Some(0), Some(2))
    );
    assert_eq!(
        (second["pulled"].as_u64(), second["pushed"].as_u64()),
        (Some(2), Some(1))
    );
    assert_eq!(
        (third["pulled"].as_u64(), third["pushed"].as_u64()),
        (Some(1), Some(0))
    );
    let (texts_a, texts_b) = match (
        a.json(&["query", "--text", "from"]),
        b.json(&["query", "--text", "from"]),
    ) {
        (Ok(x), Ok(y)) => (
            strings(&x, "/records", "/text"),
            strings(&y, "/records", "/text"),
        ),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    let mut sorted_a = texts_a.clone();
    sorted_a.sort();
    let mut sorted_b = texts_b;
    sorted_b.sort();
    assert_eq!(sorted_a, sorted_b);
    assert_eq!(sorted_a.len(), 3);

    let commits_before = match git(Path::new(&remote), &["rev-list", "--count", "main"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let noop = match b.json(&["sync"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let commits_after = match git(Path::new(&remote), &["rev-list", "--count", "main"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(noop["pushed"], 0);
    assert!(noop["commit"].is_null());
    assert_eq!(commits_before, commits_after);

    let manifest = match remote_file(&remote, "sillok/manifest.json") {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(manifest.contains("\"format\": \"sillok-archive\""));
    let months = match git(
        Path::new(&remote),
        &["ls-tree", "--name-only", "main", "sillok/events/"],
    ) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if months.lines().count() != 1 || !months.trim().ends_with(".jsonl") {
        return Err(fail(format!("expected one month file, got {months}")));
    }
    Ok(())
}

#[test]
fn only_touched_months_are_rewritten_and_unknown_events_survive() -> TestResult {
    let host = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let remote = match bare_remote(&host) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let (a, b) = match (replica(&remote), replica(&remote)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    // An old-month event and a type from a future sillok, imported as JSONL.
    let old = r#"{"event_id":"01900000-0000-7000-8000-000000000001","event_at":"2024-06-01T10:00:00.000Z","recorded_at":"2024-06-01T10:00:00.000Z","actor":"past","kind":{"type":"task_recorded","record_id":"01900000-0000-7000-8000-0000000000aa","text":"old month task","status":"completed"}}"#;
    let future = r#"{"event_id":"01900000-0000-7000-8000-000000000002","event_at":"2024-06-02T10:00:00.000Z","recorded_at":"2024-06-02T10:00:00.000Z","actor":"future","kind":{"type":"record_starred","record_id":"01900000-0000-7000-8000-0000000000aa","stars":5},"extra":true}"#;
    let jsonl = a.path("seed.jsonl");
    if let Err(error) = std::fs::write(&jsonl, format!("{old}\n{future}\n")) {
        return Err(Box::new(error));
    }
    let steps = [
        a.ok(&["import", &jsonl.display().to_string()]).map(|_| ()),
        a.ok(&["sync"]).map(|_| ()),
        a.ok(&["note", "current month"]).map(|_| ()),
        a.ok(&["sync"]).map(|_| ()),
        b.ok(&["sync"]).map(|_| ()),
    ];
    for step in steps {
        if let Err(error) = step {
            return Err(error);
        }
    }
    let touched = match git(
        Path::new(&remote),
        &["show", "--name-only", "--format=", "main"],
    ) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(
        !touched.contains("2024-06"),
        "last commit rewrote the old month: {touched}"
    );
    let june = match remote_file(&remote, "sillok/events/2024-06.jsonl") {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(
        june.lines().any(|line| line == future),
        "unknown event bytes changed"
    );
    let doctor = match b.json(&["doctor"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(doctor["unknown_events"], 1);
    assert_eq!(doctor["valid"], true);
    Ok(())
}
