//! Background sync started by `status`.

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;
use support::{Sandbox, TestResult, fail, git, run_with};

const AUTO_ON: [(&str, &str); 1] = [("SILLOK_AUTOSYNC", "1")];

fn bare_remote(sandbox: &Sandbox) -> Result<String, Box<dyn std::error::Error>> {
    let remote = sandbox.path("remote.git").display().to_string();
    match git(
        sandbox.dir.path(),
        &["init", "-q", "--bare", "-b", "main", &remote],
    ) {
        Ok(_) => Ok(remote),
        Err(error) => Err(error),
    }
}

/// `status` with background sync allowed.
fn status(sandbox: &Sandbox) -> Result<Value, Box<dyn std::error::Error>> {
    let output = match run_with(&sandbox.store, sandbox.dir.path(), &["status"], &AUTO_ON) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    match serde_json::from_slice(&output.stdout) {
        Ok(value) => Ok(value),
        Err(error) => Err(fail(format!("bad status JSON: {error}"))),
    }
}

fn state(sandbox: &Sandbox) -> Option<Value> {
    match std::fs::read(sandbox.path("sillok.db.sync.state.json")) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(_) => None,
        },
        Err(_) => None,
    }
}

/// Waits for the background sync to report.
fn finished(sandbox: &Sandbox) -> Result<Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Some(value) = state(sandbox)
            && value.get("ok").is_some()
        {
            return Ok(value);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(fail("the background sync never reported"))
}

#[test]
fn status_syncs_in_the_background_once_per_interval() -> TestResult {
    let (host, replica) = match (Sandbox::new(), Sandbox::new()) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    let remote = match bare_remote(&host) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = replica.ok(&["sync", "remote", "set", &remote]) {
        return Err(error);
    }
    if let Err(error) = replica.ok(&["note", "pushed without asking"]) {
        return Err(error);
    }
    // Without the opt-out, the first status starts a sync and returns clean.
    let first = match status(&replica) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(first.get("warnings").is_none());
    let done = match finished(&replica) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(done["ok"], true);
    assert_eq!(done["pushed"], 1);
    let events = match git(
        Path::new(&remote),
        &["ls-tree", "-r", "--name-only", "main"],
    ) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(events.contains("sillok/manifest.json"));
    // Inside the interval, another status starts nothing.
    if let Err(error) = status(&replica) {
        return Err(error);
    }
    match state(&replica) {
        Some(again) => assert_eq!(again["attempted_at"], done["attempted_at"]),
        None => return Err(fail("state file disappeared")),
    }
    let shown = match replica.json(&["sync", "remote", "show"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(shown["auto_minutes"], 60);
    assert_eq!(shown["background"]["ok"], true);
    Ok(())
}

#[test]
fn a_failed_background_sync_is_reported_by_the_next_status() -> TestResult {
    let replica = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let missing = replica.path("no-such-remote.git").display().to_string();
    if let Err(error) = replica.ok(&["sync", "remote", "set", &missing]) {
        return Err(error);
    }
    if let Err(error) = status(&replica) {
        return Err(error);
    }
    let done = match finished(&replica) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(done["ok"], false);
    let next = match status(&replica) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let notice = match next["warnings"][0].as_str() {
        Some(value) => value.to_string(),
        None => return Err(fail("expected a failure notice")),
    };
    assert!(notice.contains("the last background sync failed (sync_git_error"));
    Ok(())
}

#[test]
fn an_interval_of_zero_or_no_remote_starts_nothing() -> TestResult {
    let (host, replica, plain) = match (Sandbox::new(), Sandbox::new(), Sandbox::new()) {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => return Err(error),
    };
    let remote = match bare_remote(&host) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    for args in [
        ["sync", "remote", "set", remote.as_str()].as_slice(),
        ["sync", "auto", "0"].as_slice(),
    ] {
        if let Err(error) = replica.ok(args) {
            return Err(error);
        }
    }
    // The interval is kept beside the remote, which is unchanged.
    let shown = match replica.json(&["sync", "remote", "show"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(shown["auto_minutes"], 0);
    assert_eq!(shown["config"]["url"], remote.as_str());
    for sandbox in [&replica, &plain] {
        if let Err(error) = status(sandbox) {
            return Err(error);
        }
        assert!(state(sandbox).is_none());
    }
    Ok(())
}
