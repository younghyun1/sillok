//! `doctor` catches a projection that drifted from the event log.

mod support;

use support::{Sandbox, TestResult};

#[test]
fn tampered_projection_is_detected_and_repaired() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = sandbox.ok(&["note", "original text"]) {
        return Err(error);
    }
    let conn = match rusqlite::Connection::open(&sandbox.store) {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    if let Err(error) = conn.execute("UPDATE record SET record_text = 'tampered'", []) {
        return Err(Box::new(error));
    }
    drop(conn);
    let broken = match sandbox.json(&["doctor"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(broken["valid"], false);
    assert_eq!(broken["mismatch_count"], 1);
    let repaired = match sandbox.json(&["doctor", "--repair"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(repaired["valid"], true);
    let found = match sandbox.json(&["query", "--text", "original"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(found["records"].as_array().map(Vec::len), Some(1));
    Ok(())
}

#[test]
fn missing_store_is_valid_and_untouched() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let report = match sandbox.json(&["doctor"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(report["missing"], true);
    assert!(
        !sandbox.store.exists(),
        "read commands must not create the store"
    );
    Ok(())
}
