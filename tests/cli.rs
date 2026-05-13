use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

fn temp_store() -> (tempfile::TempDir, PathBuf) {
    let dir = match tempfile::tempdir() {
        Ok(value) => value,
        Err(error) => panic!("tempdir failed: {error}"),
    };
    let store = dir.path().join("sillok.slk.zst");
    (dir, store)
}

fn run_json(store: &Path, args: &[&str]) -> Value {
    let mut command = match Command::cargo_bin("sillok") {
        Ok(value) => value,
        Err(error) => panic!("binary lookup failed: {error}"),
    };
    command.arg("--store").arg(store);
    for arg in args {
        command.arg(arg);
    }
    let output = match command.output() {
        Ok(value) => value,
        Err(error) => panic!("command failed to run: {error}"),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        panic!("command failed: stdout={stdout} stderr={stderr}");
    }
    match serde_json::from_slice::<Value>(&output.stdout) {
        Ok(value) => value,
        Err(error) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            panic!("json parse failed: {error}; stdout={stdout}");
        }
    }
}

fn string_at<'a>(value: &'a Value, path: &str) -> &'a str {
    match value.pointer(path) {
        Some(node) => match node.as_str() {
            Some(text) => text,
            None => panic!("json path `{path}` is not a string"),
        },
        None => panic!("json path `{path}` missing"),
    }
}

#[test]
fn records_note_and_reads_day_tree() {
    let (_dir, store) = temp_store();
    let note = run_json(
        &store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T10:00:00Z",
            "note",
            "Implemented archive storage",
            "--tags",
            "rust,storage",
        ],
    );
    assert_eq!(note["ok"], true);
    let task_id = string_at(&note, "/ids/task_id").to_string();

    let day = run_json(&store, &["--tz", "UTC", "day", "--date", "2026-05-13"]);
    assert_eq!(day["ok"], true);
    assert_eq!(day["data"]["records"][0]["record_id"], task_id);

    let show = run_json(&store, &["show", &task_id]);
    assert_eq!(show["ok"], true);
    assert_eq!(
        show["data"]["record"]["text"],
        "Implemented archive storage"
    );
}

#[test]
fn completes_objective_and_truncates_with_backup() {
    let (_dir, store) = temp_store();
    let objective = run_json(
        &store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T08:00:00Z",
            "objective",
            "add",
            "Finish the Sillok CLI",
        ],
    );
    let objective_id = string_at(&objective, "/ids/objective_id").to_string();

    let complete = run_json(&store, &["objective", "complete", &objective_id]);
    assert_eq!(complete["data"]["record"]["status"], "completed");

    let truncate = run_json(&store, &["truncate", "--yes"]);
    let backup = string_at(&truncate, "/data/backup");
    assert!(Path::new(backup).exists());

    let day = run_json(&store, &["--tz", "UTC", "day", "--date", "2026-05-13"]);
    assert_eq!(day["data"]["records"].as_array().map(Vec::len), Some(0));
}
