//! Id prefixes, batch notes, and doctor's leftovers.

mod support;

use support::{Sandbox, TestResult, fail, run_input, strings};

#[test]
fn a_unique_id_prefix_names_a_record() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let (objective, task) = match (
        sandbox.ok(&[
            "--at",
            "2026-01-01T00:00:00Z",
            "objective",
            "add",
            "Old objective",
        ]),
        sandbox.ok(&["note", "Recent task"]),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    // Reads, writes, and parents all take a prefix.
    let shown = match sandbox.json(&["show", &task[..13]]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(shown["record"]["id"], task.as_str());
    let moved = match sandbox.ok(&["move", &task[..13], "--parent", &objective[..13]]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(moved, task);
    let completed = match sandbox.ok(&["objective", "complete", &objective.replace('-', "")[..12]])
    {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(completed, objective);
    // Too short, unknown, and shared prefixes each fail with their own code.
    for (prefix, code) in [
        (&task[..6], "invalid_id"),
        ("ffffffff-ffff", "record_not_found"),
    ] {
        match sandbox.error(&["show", prefix]) {
            Ok((_, error)) => assert_eq!(error["error"], code),
            Err(error) => return Err(error),
        }
    }
    // Ids start with their creation time, so records made together share a
    // short prefix. Three notes span at most one rollover of the eighth
    // digit, so two of them always share eight.
    let mut made = Vec::new();
    for text in ["one", "two", "three"] {
        match sandbox.ok(&["note", text]) {
            Ok(id) => made.push(id),
            Err(error) => return Err(error),
        }
    }
    let shared = match (made[0][..8] == made[1][..8], made[1][..8] == made[2][..8]) {
        (true, _) => &made[0][..8],
        (false, true) => &made[1][..8],
        (false, false) => return Err(fail("three ids in a row shared no prefix")),
    };
    match sandbox.error(&["show", shared]) {
        Ok((_, error)) => assert_eq!(error["error"], "ambiguous_id"),
        Err(error) => return Err(error),
    }
    Ok(())
}

#[test]
fn notes_from_stdin_are_recorded_together_or_not_at_all() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let objective = match sandbox.ok(&["objective", "add", "Batch target"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let lines = format!(
        "{{\"text\":\"first\"}}\n\n{{\"text\":\"second\",\"tags\":[\"own\"],\"status\":\"active\"}}\n{{\"text\":\"third\",\"parent\":\"{objective}\"}}\n"
    );
    let output = match run_input(
        &sandbox.store,
        sandbox.dir.path(),
        &[
            "note",
            "--stdin",
            "--parent",
            &objective[..13],
            "--tags",
            "shared",
        ],
        &[],
        Some(&lines),
    ) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if !output.status.success() {
        return Err(fail(String::from_utf8_lossy(&output.stderr).to_string()));
    }
    let printed = String::from_utf8_lossy(&output.stdout);
    assert_eq!(printed.lines().count(), 3);
    let tree = match sandbox.json(&["tree", &objective]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let children = &tree["records"][0]["children"];
    assert_eq!(strings(&tree, "/records/0/children", "/text").len(), 3);
    let second = match children
        .as_array()
        .and_then(|list| list.iter().find(|child| child["text"] == "second"))
    {
        Some(value) => value,
        None => return Err(fail("second task missing")),
    };
    assert_eq!(second["status"], "active");
    assert_eq!(second["tags"][0], "own");
    // One bad line rejects the whole batch.
    let bad = "{\"text\":\"kept out\"}\n{\"text\":\"\",\"oops\":1}\n";
    let rejected = match run_input(
        &sandbox.store,
        sandbox.dir.path(),
        &["note", "--stdin"],
        &[],
        Some(bad),
    ) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("invalid_batch_line"));
    let after = match sandbox.json(&["query", "--text", "kept out"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(after["records"].as_array().map(Vec::len), Some(0));
    Ok(())
}

#[test]
fn doctor_reports_and_prunes_backups() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = sandbox.ok(&["note", "keep me"]) {
        return Err(error);
    }
    for name in [
        "sillok.db.v2-1.bak.db",
        "sillok.1779263353131.019e445c.bak.db",
        "unrelated.bak.db",
        "sillok.db.sync.json",
    ] {
        if let Err(error) = std::fs::write(sandbox.path(name), "12345") {
            return Err(Box::new(error));
        }
    }
    let before = match sandbox.json(&["doctor"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(before["leftovers"]["backups"], 2);
    assert_eq!(before["leftovers"]["backup_bytes"], 10);
    let after = match sandbox.json(&["doctor", "--prune"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(after["leftovers"]["pruned"]["backups"], 2);
    assert_eq!(after["leftovers"]["backups"], 0);
    assert_eq!(after["valid"], true);
    // Only backups of this store went; the store and its sidecar stay.
    assert!(!sandbox.path("sillok.db.v2-1.bak.db").exists());
    assert!(sandbox.path("unrelated.bak.db").exists());
    assert!(sandbox.path("sillok.db.sync.json").exists());
    assert!(sandbox.store.exists());
    Ok(())
}
