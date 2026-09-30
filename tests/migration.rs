//! 0.9/0.10 data against the committed golden fixtures.

mod support;

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use support::{Sandbox, TestResult, fail, fixture};

/// Expected 1.0 view of each 0.10 record that stays visible, keyed by id.
fn expected() -> Result<HashMap<String, Value>, Box<dyn std::error::Error>> {
    let text = match std::fs::read_to_string(fixture("expected.json")) {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    let parsed: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    let records = match parsed["visible_records"].as_array() {
        Some(value) => value.clone(),
        None => return Err(fail("expected.json has no visible_records")),
    };
    let days: HashSet<String> = records
        .iter()
        .filter(|record| record["kind"] == "day")
        .filter_map(|record| record["record_id"].as_str().map(str::to_string))
        .collect();
    Ok(records
        .into_iter()
        .filter(|record| record["kind"] != "day")
        .filter_map(|record| {
            let id = match record["record_id"].as_str() {
                Some(value) => value.to_string(),
                None => return None,
            };
            let parent = match record["parent_id"].as_str() {
                Some(parent) if !days.contains(parent) => Value::String(parent.to_string()),
                _ => Value::Null,
            };
            let mut view = serde_json::json!({
                "kind": record["kind"],
                "status": record["status"],
                "text": record["text"],
                "parent": parent,
            });
            // 0.10 stored the objective completion note in `purpose`; 1.0 moves it to `note`.
            let field = if record["kind"] == "objective" {
                "note"
            } else {
                "purpose"
            };
            view[field] = record["purpose"].clone();
            Some((id, view))
        })
        .collect())
}

fn compare(records: &Value) -> TestResult {
    let wanted = match expected() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let found = match records.as_array() {
        Some(value) => value,
        None => return Err(fail("no records")),
    };
    assert_eq!(found.len(), wanted.len(), "visible record count");
    for record in found {
        let id = match record["id"].as_str() {
            Some(value) => value,
            None => return Err(fail("record without id")),
        };
        let want = match wanted.get(id) {
            Some(value) => value,
            None => return Err(fail(format!("unexpected record {id}"))),
        };
        for key in ["kind", "status", "text", "parent", "note", "purpose"] {
            if want.get(key).is_some() {
                let actual = match record.get(key) {
                    Some(value) => value,
                    None => &Value::Null,
                };
                assert_eq!(actual, &want[key], "{id} {key}");
            }
        }
    }
    Ok(())
}

#[test]
fn v2_store_migrates_in_place_on_first_use() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = std::fs::copy(fixture("store.db"), &sandbox.store) {
        return Err(Box::new(error));
    }
    let output = match sandbox.run(&["query", "--since", "2000-01-01", "--full"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(output.status.success());
    let stdout: Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    let warnings = stdout["warnings"].to_string();
    assert!(warnings.contains("migrated 0.10 store"), "{warnings}");
    if let Err(error) = compare(&stdout["records"]) {
        return Err(error);
    }
    // Credentials in 0.10 remotes are stripped during conversion.
    assert!(!stdout.to_string().contains("secret"));
    let backups = match std::fs::read_dir(sandbox.dir.path()) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".v2-"))
            .count(),
        Err(error) => return Err(Box::new(error)),
    };
    assert_eq!(backups, 1);
    let again = match sandbox.json(&["doctor"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(again["valid"], true);
    assert!(
        again.get("warnings").is_none(),
        "second open must not migrate again"
    );
    Ok(())
}

#[test]
fn every_legacy_source_converts_to_identical_events() -> TestResult {
    let mut exports = Vec::new();
    for source in ["store.db", "archive.slk.zst", "sync.slk.zst"] {
        let sandbox = match Sandbox::new() {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let path = fixture(source).display().to_string();
        let report = match sandbox.json(&["import", &path]) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        // 1 archive marker, 3 day openings, and the amend that retracted a day.
        assert_eq!(report["dropped"], 5, "{source}");
        match sandbox.json(&["query", "--since", "2000-01-01"]) {
            Ok(found) => {
                if let Err(error) = compare(&found["records"]) {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
        match sandbox.ok(&["export", "--events"]) {
            Ok(text) => {
                let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
                lines.sort();
                exports.push(lines);
            }
            Err(error) => return Err(error),
        }
    }
    // Byte-identical conversion is what lets independently migrated replicas sync.
    assert_eq!(exports[0], exports[1]);
    assert_eq!(exports[1], exports[2]);
    Ok(())
}

#[test]
fn import_is_idempotent_and_dry_run_writes_nothing() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let path = fixture("archive.slk.zst").display().to_string();
    let (dry, first, second) = match (
        sandbox.json(&["import", &path, "--dry-run"]),
        sandbox.json(&["import", &path]),
        sandbox.json(&["import", &path]),
    ) {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => return Err(error),
    };
    assert_eq!(dry["added"], first["added"]);
    assert_eq!(second["added"], 0);
    Ok(())
}

#[test]
fn old_migrate_spelling_reads_the_legacy_file_beside_the_store() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = std::fs::copy(fixture("archive.slk.zst"), sandbox.path("sillok.slk.zst")) {
        return Err(Box::new(error));
    }
    let report = match sandbox.json(&["migrate", "--yes"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(report["format"], "legacy_archive");
    assert_eq!(report["added"], 11);
    Ok(())
}
