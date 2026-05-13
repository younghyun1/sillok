use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

fn boxed_error(message: String) -> Box<dyn std::error::Error> {
    Box::new(std::io::Error::other(message))
}

fn temp_store() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let dir = match tempfile::tempdir() {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    let store = dir.path().join("sillok.slk.zst");
    Ok((dir, store))
}

fn run_json(store: &Path, args: &[&str]) -> Result<Value, Box<dyn std::error::Error>> {
    let stdout = run_stdout(store, args)?;
    match serde_json::from_str::<Value>(&stdout) {
        Ok(value) => Ok(value),
        Err(error) => Err(boxed_error(format!(
            "json parse failed: {error}; stdout={stdout}"
        ))),
    }
}

fn run_stdout(store: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let mut command = match Command::cargo_bin("sillok") {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    command.arg("--store").arg(store);
    command.env("TZ", "UTC");
    for arg in args {
        command.arg(arg);
    }
    let output = match command.output() {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(boxed_error(format!(
            "command failed: stdout={stdout} stderr={stderr}"
        )));
    }
    match String::from_utf8(output.stdout) {
        Ok(value) => Ok(value),
        Err(error) => Err(Box::new(error)),
    }
}

fn string_at<'a>(value: &'a Value, path: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    match value.pointer(path) {
        Some(node) => match node.as_str() {
            Some(text) => Ok(text),
            None => Err(boxed_error(format!("json path `{path}` is not a string"))),
        },
        None => Err(boxed_error(format!("json path `{path}` missing"))),
    }
}

#[test]
fn records_note_and_reads_day_tree() -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, store) = temp_store()?;
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
    )?;
    assert_eq!(note["ok"], true);
    string_at(&note, "/generated_at")?;
    assert_eq!(
        note["data"]["record"]["created_at"],
        "2026-05-13T10:00:00+00:00"
    );
    let task_id = string_at(&note, "/ids/task_id")?.to_string();

    let day = run_json(&store, &["--tz", "UTC", "day", "--date", "2026-05-13"])?;
    assert_eq!(day["ok"], true);
    string_at(&day, "/generated_at")?;
    assert_eq!(day["data"]["records"][0]["record_id"], task_id);

    let human = run_stdout(
        &store,
        &["--human", "--tz", "UTC", "day", "--date", "2026-05-13"],
    )?;
    assert!(human.contains("2026-05-13 (UTC) - 1 record"));
    assert!(human.contains("[completed task] Implemented archive storage (2026-05-13 10:00 AM"));
    assert!(!human.contains("2026-05-13T10:00:00+00:00"));
    assert!(human.contains("tags: rust, storage"));

    let show = run_json(&store, &["show", &task_id])?;
    assert_eq!(show["ok"], true);
    assert_eq!(
        show["data"]["record"]["text"],
        "Implemented archive storage"
    );
    let show_human = run_stdout(&store, &["--human", "show", &task_id])?;
    assert!(show_human.contains("created: 2026-05-13 10:00 AM"));
    assert!(show_human.contains("Events"));
    Ok(())
}

#[test]
fn completes_objective_and_truncates_with_backup() -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, store) = temp_store()?;
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
    )?;
    let objective_id = string_at(&objective, "/ids/objective_id")?.to_string();

    let complete = run_json(&store, &["objective", "complete", &objective_id])?;
    assert_eq!(complete["data"]["record"]["status"], "completed");

    let truncate = run_json(&store, &["truncate", "--yes"])?;
    let backup = string_at(&truncate, "/data/backup")?;
    assert!(Path::new(backup).exists());

    let day = run_json(&store, &["--tz", "UTC", "day", "--date", "2026-05-13"])?;
    assert_eq!(day["data"]["records"].as_array().map(Vec::len), Some(0));
    Ok(())
}
