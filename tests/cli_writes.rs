//! Write commands: output shape, validation, and 0.10 aliases.

mod support;

use support::{Sandbox, TestResult, fail};

#[test]
fn writes_print_only_the_id() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let id = match sandbox.ok(&["note", "Shipped the lock fix"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if id.len() != 36 || id.contains('\n') {
        return Err(fail(format!("expected a bare id, got `{id}`")));
    }
    let shown = match sandbox.json(&["show", &id]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(shown["record"]["text"], "Shipped the lock fix");
    assert_eq!(shown["record"]["status"], "completed");
    Ok(())
}

#[test]
fn objective_lifecycle_and_amend_fields() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let steps = (
        sandbox.ok(&["objective", "add", "Ship 1.0", "--tags", "Release"]),
        sandbox.ok(&["note", "placeholder", "--purpose", "why", "--tags", "a,b"]),
    );
    let (objective, task) = match steps {
        (Ok(o), Ok(t)) => (o, t),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    for args in [
        vec![
            "amend",
            task.as_str(),
            "--text",
            "real text",
            "--clear-purpose",
            "--clear-tags",
        ],
        vec!["move", task.as_str(), "--parent", objective.as_str()],
        vec![
            "objective",
            "complete",
            objective.as_str(),
            "--note",
            "done",
        ],
    ] {
        if let Err(error) = sandbox.ok(&args) {
            return Err(error);
        }
    }
    let (task_view, objective_view) = match (
        sandbox.json(&["show", &task]),
        sandbox.json(&["show", &objective]),
    ) {
        (Ok(t), Ok(o)) => (t, o),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    assert_eq!(task_view["record"]["text"], "real text");
    assert!(task_view["record"].get("purpose").is_none());
    assert!(task_view["record"].get("tags").is_none());
    assert_eq!(task_view["record"]["parent"], objective.as_str());
    assert_eq!(objective_view["record"]["status"], "completed");
    assert_eq!(objective_view["record"]["note"], "done");
    assert_eq!(objective_view["record"]["tags"][0], "release");
    Ok(())
}

#[test]
fn moves_that_create_cycles_are_rejected() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let parent = match sandbox.ok(&["note", "parent"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let child = match sandbox.ok(&["note", "child", "--parent", &parent]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let (code, error) = match sandbox.error(&["move", &parent, "--parent", &child]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(code, 1);
    assert_eq!(error["error"], "parent_cycle");
    Ok(())
}

#[test]
fn retract_and_restore() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let id = match sandbox.ok(&["note", "oops"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = sandbox.ok(&["retract", &id, "--reason", "wrong place"]) {
        return Err(error);
    }
    let (_, amend_error) = match sandbox.error(&["amend", &id, "--text", "x"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(amend_error["error"], "record_retracted");
    let hidden = match sandbox.json(&["query", "--text", "oops"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(hidden["records"].as_array().map(Vec::len), Some(0));
    if let Err(error) = sandbox.ok(&["restore", &id]) {
        return Err(error);
    }
    let shown = match sandbox.json(&["show", &id]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(shown["record"]["status"], "completed");
    assert!(shown["record"].get("retraction_reason").is_none());
    Ok(())
}

#[test]
fn invalid_input_is_json_on_stderr() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let checks = [
        (
            vec!["note", "x", "--status", "retracted"],
            1,
            "invalid_status",
        ),
        (vec!["note", "   "], 1, "invalid_text"),
        (vec!["note", "x", "--tags", "has space"], 1, "invalid_tag"),
        (vec!["show", "not-an-id"], 1, "invalid_id"),
        (vec!["--tz", "Mars/Base", "day"], 1, "invalid_timezone"),
        (vec!["no-such-command"], 2, "usage"),
    ];
    for (args, expected_code, expected_error) in checks {
        let (code, error) = match sandbox.error(&args) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        assert_eq!(
            (code, error["error"].as_str()),
            (expected_code, Some(expected_error)),
            "{args:?}"
        );
    }
    Ok(())
}

#[test]
fn old_spellings_still_work() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let id = match sandbox.ok(&["note", "alias check"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let tree = match sandbox.json(&["tree", "--root", &id]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(tree["records"][0]["id"], id.as_str());
    let exported = match sandbox.ok(&["export", "json"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(exported.lines().count(), 1);
    let archive = match sandbox.ok(&["truncate", "--yes"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(archive.len(), 36);
    let empty = match sandbox.json(&["query", "--since", "2000-01-01"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(empty["records"].as_array().map(Vec::len), Some(0));
    Ok(())
}
