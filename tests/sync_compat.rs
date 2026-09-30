//! Sync against 0.10 remotes and diverging event bytes.

mod support;

use std::path::Path;

use support::{Sandbox, TestResult, fixture, git};

#[test]
fn legacy_remote_artifact_is_imported_and_replaced() -> TestResult {
    let host = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let remote = host.path("remote.git").display().to_string();
    let seed = host.path("seed");
    let seed_arg = seed.display().to_string();
    let artifact = fixture("sync.slk.zst");
    for args in [
        vec!["init", "-q", "--bare", "-b", "main", remote.as_str()],
        vec!["init", "-q", "-b", "main", seed_arg.as_str()],
    ] {
        if let Err(error) = git(host.dir.path(), &args) {
            return Err(error);
        }
    }
    if let Err(error) = std::fs::copy(&artifact, seed.join("sillok.slk.zst")) {
        return Err(Box::new(error));
    }
    for args in [
        vec!["add", "sillok.slk.zst"],
        vec![
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "0.10 artifact",
        ],
        vec!["push", "-q", remote.as_str(), "main"],
    ] {
        if let Err(error) = git(&seed, &args) {
            return Err(error);
        }
    }
    let replica = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    // A 0.10 sidecar, as an upgraded machine would have it.
    let sidecar = format!(
        "{{\"schema_version\":1,\"url\":\"{remote}\",\"branch\":\"main\",\"path\":\"sillok.slk.zst\"}}"
    );
    if let Err(error) = std::fs::write(replica.path("sillok.db.sync.json"), sidecar) {
        return Err(Box::new(error));
    }
    let report = match replica.json(&["sync"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(report["legacy_imported"], 11);
    assert_eq!(report["pulled"], 11);
    let files = match git(
        Path::new(&remote),
        &["ls-tree", "-r", "--name-only", "main"],
    ) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert!(
        !files.contains("sillok.slk.zst"),
        "legacy artifact still present: {files}"
    );
    assert!(files.contains("sillok/manifest.json"));
    let config = match replica.json(&["sync", "remote", "show"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(config["config"]["schema_version"], 2);
    let found = match replica.json(&["query", "--since", "2000-01-01"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(found["records"].as_array().map(Vec::len), Some(5));
    // The config keeps legacy_path; a later sync with new events must still
    // push even though the artifact no longer exists on the remote.
    if let Err(error) = replica.ok(&["note", "after the legacy import"]) {
        return Err(error);
    }
    let later = match replica.json(&["sync"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(later["pushed"], 1);
    assert_eq!(later["legacy_imported"], 0);
    Ok(())
}

#[test]
fn diverging_bytes_converge_without_blocking() -> TestResult {
    let host = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let remote = host.path("remote.git").display().to_string();
    if let Err(error) = git(
        host.dir.path(),
        &["init", "-q", "--bare", "-b", "main", remote.as_str()],
    ) {
        return Err(error);
    }
    let line = |actor: &str| {
        format!(
            r#"{{"event_id":"01900000-0000-7000-8000-00000000000c","event_at":"2026-09-01T10:00:00.000Z","recorded_at":"2026-09-01T10:00:00.000Z","actor":"{actor}","kind":{{"type":"task_recorded","record_id":"01900000-0000-7000-8000-0000000000cc","text":"same id","status":"completed"}}}}"#
        )
    };
    let mut replicas = Vec::new();
    for actor in ["zeta", "alpha"] {
        let replica = match Sandbox::new() {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let seed = replica.path("seed.jsonl");
        if let Err(error) = std::fs::write(&seed, line(actor)) {
            return Err(Box::new(error));
        }
        for args in [
            vec!["sync", "remote", "set", remote.as_str()],
            vec!["import", &seed.display().to_string()],
        ] {
            if let Err(error) = replica.ok(&args) {
                return Err(error);
            }
        }
        replicas.push(replica);
    }
    for replica in replicas.iter().chain(replicas.iter()) {
        if let Err(error) = replica.ok(&["sync"]) {
            return Err(error);
        }
    }
    for replica in &replicas {
        let exported = match replica.ok(&["export", "--events"]) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        assert_eq!(
            exported,
            line("alpha"),
            "replicas must converge on the smaller bytes"
        );
    }
    Ok(())
}
