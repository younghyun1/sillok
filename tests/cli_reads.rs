//! Read commands: derived days, queries, and status.

mod support;

use support::{Sandbox, TestResult, strings};

#[test]
fn a_task_appears_on_every_day_it_was_worked() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let objective = match sandbox.ok(&[
        "--at",
        "2026-09-28T09:00:00Z",
        "objective",
        "add",
        "Long objective",
    ]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let task = match sandbox.ok(&[
        "--at",
        "2026-09-28T10:00:00Z",
        "note",
        "Multi-day task",
        "--parent",
        &objective,
        "--status",
        "active",
    ]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = sandbox.ok(&[
        "--at",
        "2026-09-30T15:00:00Z",
        "amend",
        &task,
        "--status",
        "done",
    ]) {
        return Err(error);
    }
    let (first, middle, last) = match (
        sandbox.json(&["day", "--date", "2026-09-28"]),
        sandbox.json(&["day", "--date", "2026-09-29"]),
        sandbox.json(&["day", "--date", "2026-09-30"]),
    ) {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => return Err(error),
    };
    assert_eq!(first["records"][0]["id"], objective.as_str());
    assert_eq!(
        first["records"][0]["children"][0]["activity"][0],
        "recorded"
    );
    assert_eq!(middle["records"].as_array().map(Vec::len), Some(0));
    // On the last day the objective is context only (no activity); the task completed.
    let root = &last["records"][0];
    assert_eq!(root["id"], objective.as_str());
    assert!(root.get("activity").is_none());
    assert_eq!(root["children"][0]["activity"][0], "completed");
    Ok(())
}

#[test]
fn day_windows_follow_the_timezone() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    // 04:30 UTC on the 30th is still the 29th in Denver.
    if let Err(error) = sandbox.ok(&["--at", "2026-09-30T04:30:00Z", "note", "late night"]) {
        return Err(error);
    }
    let (utc, denver) = match (
        sandbox.json(&["day", "--date", "2026-09-30"]),
        sandbox.json(&["--tz", "America/Denver", "day", "--date", "2026-09-29"]),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    assert_eq!(utc["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(denver["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(denver["tz"], "America/Denver");
    Ok(())
}

#[test]
fn query_filters_and_limits() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    for (at, text, tags, status) in [
        (
            "2026-09-01T10:00:00Z",
            "alpha sync work",
            "sync,rust",
            "completed",
        ),
        ("2026-09-02T10:00:00Z", "beta docs", "docs", "active"),
        ("2026-09-03T10:00:00Z", "gamma sync fix", "sync", "blocked"),
    ] {
        if let Err(error) =
            sandbox.ok(&["--at", at, "note", text, "--tags", tags, "--status", status])
        {
            return Err(error);
        }
    }
    let cases: [(&[&str], Vec<&str>); 6] = [
        (
            &["query", "--tag", "sync"],
            vec!["alpha sync work", "gamma sync fix"],
        ),
        (&["query", "--tag", "sync,rust"], vec!["alpha sync work"]),
        (&["query", "--open"], vec!["beta docs", "gamma sync fix"]),
        (
            &["query", "--text", "SYNC", "--limit", "1"],
            vec!["gamma sync fix"],
        ),
        (&["query", "--date", "2026-09-02"], vec!["beta docs"]),
        (
            &[
                "query",
                "--since",
                "2026-09-02T00:00:00Z",
                "--status",
                "blocked",
            ],
            vec!["gamma sync fix"],
        ),
    ];
    for (args, expected) in cases {
        let found = match sandbox.json(args) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        assert_eq!(strings(&found, "/records", "/text"), expected, "{args:?}");
    }
    Ok(())
}

#[test]
fn status_lists_open_objectives_and_recent_work() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let objective = match sandbox.ok(&["objective", "add", "Resume me"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = sandbox.ok(&["note", "did a thing", "--parent", &objective]) {
        return Err(error);
    }
    let status = match sandbox.json(&["status"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(
        strings(&status, "/objectives", "/id"),
        vec![objective.clone()]
    );
    assert_eq!(strings(&status, "/recent", "/text")[0], "did a thing");
    let listed = match sandbox.json(&["objective", "list"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(strings(&listed, "/records", "/id"), vec![objective]);
    Ok(())
}

#[test]
fn output_modes() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let id = match sandbox.ok(&["note", "mode check"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let (envelope, human, compact) = match (
        sandbox.json(&["--json", "show", &id]),
        sandbox.ok(&["--human", "day"]),
        sandbox.json(&["query", "--text", "mode"]),
    ) {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => return Err(error),
    };
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["data"]["record"]["id"], id.as_str());
    assert!(human.contains("mode check"));
    assert!(compact["records"][0].get("context").is_none());
    Ok(())
}
