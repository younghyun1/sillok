use std::path::{Path, PathBuf};

mod sync_support;

use sync_support::{
    boxed_error, clone_remote, init_bare_remote, remote_set, run_failure_json, run_json, temp_store,
};

#[test]
fn sync_remote_set_and_show_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, store) = temp_store()?;
    let remote = init_bare_remote(dir.path())?;
    let set = remote_set(&store, &remote)?;
    assert_eq!(set["command"], "sync");
    assert_eq!(set["data"]["action"], "remote_set");
    assert_eq!(set["data"]["remote"]["branch"], "main");
    assert!(PathBuf::from(format!("{}.sync.json", store.display())).exists());

    let show = run_json(&store, &["sync", "remote", "show"])?;
    assert_eq!(show["data"]["action"], "remote_show");
    assert_eq!(show["data"]["remote"]["url"], remote.display().to_string());
    Ok(())
}

#[test]
fn sync_push_creates_remote_artifact() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, store) = temp_store()?;
    let remote = init_bare_remote(dir.path())?;
    remote_set(&store, &remote)?;
    run_json(
        &store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T08:00:00Z",
            "note",
            "sync push seed",
        ],
    )?;

    let pushed = run_json(&store, &["sync", "push"])?;
    assert_eq!(pushed["data"]["pushed"], true);
    assert!(pushed["data"]["commit"].is_string());

    let checkout = dir.path().join("checkout");
    clone_remote(&remote, &checkout)?;
    assert!(checkout.join("sillok.slk.zst").exists());
    Ok(())
}

#[test]
fn sync_pull_creates_missing_local_db() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, source_store) = temp_store()?;
    let remote = init_bare_remote(dir.path())?;
    remote_set(&source_store, &remote)?;
    run_json(
        &source_store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T09:00:00Z",
            "note",
            "pull me from remote",
        ],
    )?;
    run_json(&source_store, &["sync", "push"])?;

    let target_store = dir.path().join("target.db");
    remote_set(&target_store, &remote)?;
    let pulled = run_json(&target_store, &["sync", "pull"])?;
    assert_eq!(pulled["data"]["pulled"], true);
    assert!(target_store.exists());

    let day = run_json(
        &target_store,
        &["--tz", "UTC", "day", "--date", "2026-05-13"],
    )?;
    assert_eq!(day["data"]["records"][0]["text"], "pull me from remote");
    Ok(())
}

#[test]
fn sync_run_merges_diverged_archives() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, left_store) = temp_store()?;
    let remote = init_bare_remote(dir.path())?;
    remote_set(&left_store, &remote)?;
    run_json(
        &left_store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T08:00:00Z",
            "note",
            "base sync note",
        ],
    )?;
    run_json(&left_store, &["sync", "push"])?;

    let right_store = dir.path().join("right.db");
    remote_set(&right_store, &remote)?;
    run_json(&right_store, &["sync", "pull"])?;
    run_json(
        &left_store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T09:00:00Z",
            "note",
            "left side note",
        ],
    )?;
    run_json(&left_store, &["sync", "push"])?;
    run_json(
        &right_store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T10:00:00Z",
            "note",
            "right side note",
        ],
    )?;

    let synced = run_json(&right_store, &["sync", "run"])?;
    assert_eq!(synced["data"]["merged"], true);
    assert_eq!(synced["data"]["pulled"], true);
    assert_eq!(synced["data"]["pushed"], true);

    let verify_store = dir.path().join("verify.db");
    remote_set(&verify_store, &remote)?;
    run_json(&verify_store, &["sync", "pull"])?;
    let day = run_json(
        &verify_store,
        &["--tz", "UTC", "day", "--date", "2026-05-13"],
    )?;
    assert_eq!(day["data"]["records"].as_array().map(Vec::len), Some(3));
    assert_eq!(day["data"]["records"][1]["text"], "left side note");
    assert_eq!(day["data"]["records"][2]["text"], "right side note");
    Ok(())
}

/// Seeds the remote with `text` from an independent source store and returns the
/// bare remote path plus the source archive id.
fn seed_remote(
    dir: &Path,
    at: &str,
    text: &str,
) -> Result<(PathBuf, String), Box<dyn std::error::Error>> {
    let remote = init_bare_remote(dir)?;
    let source = dir.join("source.db");
    remote_set(&source, &remote)?;
    run_json(&source, &["--tz", "UTC", "--at", at, "note", text])?;
    let pushed = run_json(&source, &["sync", "push"])?;
    let archive_id = pushed["data"]["remote_after"]["archive_id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| boxed_error("missing source archive id".to_string()))?;
    Ok((remote, archive_id))
}

#[test]
fn sync_pull_overwrites_independent_local_archive() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _store) = temp_store()?;
    let (remote, remote_id) =
        seed_remote(dir.path(), "2026-05-13T08:00:00Z", "remote archive note")?;

    let local_store = dir.path().join("local.db");
    remote_set(&local_store, &remote)?;
    run_json(
        &local_store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T11:00:00Z",
            "note",
            "independent local note",
        ],
    )?;

    let pulled = run_json(&local_store, &["sync", "pull"])?;
    assert_eq!(pulled["data"]["pulled"], true);
    assert_eq!(pulled["data"]["merged"], false);
    // Local adopted the remote archive id and kept a backup of its old database.
    assert_eq!(pulled["data"]["local_after"]["archive_id"], remote_id);
    assert!(pulled["data"]["backup"].is_string());

    let day = run_json(
        &local_store,
        &["--tz", "UTC", "day", "--date", "2026-05-13"],
    )?;
    assert_eq!(day["data"]["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(day["data"]["records"][0]["text"], "remote archive note");
    Ok(())
}

#[test]
fn sync_push_overwrites_remote_with_independent_local() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _store) = temp_store()?;
    let (remote, _remote_id) =
        seed_remote(dir.path(), "2026-05-13T08:00:00Z", "remote archive note")?;

    let local_store = dir.path().join("local.db");
    remote_set(&local_store, &remote)?;
    run_json(
        &local_store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T11:00:00Z",
            "note",
            "independent local note",
        ],
    )?;

    let pushed = run_json(&local_store, &["sync", "push"])?;
    assert_eq!(pushed["data"]["pushed"], true);
    assert_eq!(pushed["data"]["merged"], false);

    // A fresh store pulling the remote now sees the local archive, not the seed.
    let verify_store = dir.path().join("verify.db");
    remote_set(&verify_store, &remote)?;
    run_json(&verify_store, &["sync", "pull"])?;
    let day = run_json(
        &verify_store,
        &["--tz", "UTC", "day", "--date", "2026-05-13"],
    )?;
    assert_eq!(day["data"]["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(day["data"]["records"][0]["text"], "independent local note");
    Ok(())
}

#[test]
fn sync_pull_noop_when_remote_absent() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, store) = temp_store()?;
    let remote = init_bare_remote(dir.path())?;
    remote_set(&store, &remote)?;
    run_json(
        &store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T08:00:00Z",
            "note",
            "local only note",
        ],
    )?;

    let pulled = run_json(&store, &["sync", "pull"])?;
    assert_eq!(pulled["data"]["pulled"], false);
    assert!(pulled["data"]["backup"].is_null());

    let day = run_json(&store, &["--tz", "UTC", "day", "--date", "2026-05-13"])?;
    assert_eq!(day["data"]["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(day["data"]["records"][0]["text"], "local only note");
    Ok(())
}

#[test]
fn sync_push_errors_when_local_missing() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _store) = temp_store()?;
    let remote = init_bare_remote(dir.path())?;
    let ghost_store = dir.path().join("ghost.db");
    remote_set(&ghost_store, &remote)?;

    let failed = run_failure_json(&ghost_store, &["sync", "push"])?;
    assert_eq!(failed["error"]["code"], "archive_missing");
    Ok(())
}

#[test]
fn sync_run_mismatch_needs_choice_when_noninteractive() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _store) = temp_store()?;
    let (remote, _remote_id) =
        seed_remote(dir.path(), "2026-05-13T08:00:00Z", "remote archive note")?;

    let local_store = dir.path().join("local.db");
    remote_set(&local_store, &remote)?;
    run_json(
        &local_store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T11:00:00Z",
            "note",
            "independent local note",
        ],
    )?;

    // `--json` runs are non-interactive, so `run` refuses rather than guessing.
    let failed = run_failure_json(&local_store, &["sync", "run"])?;
    assert_eq!(failed["error"]["code"], "sync_mismatch_needs_choice");

    let day = run_json(
        &local_store,
        &["--tz", "UTC", "day", "--date", "2026-05-13"],
    )?;
    assert_eq!(day["data"]["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(day["data"]["records"][0]["text"], "independent local note");
    Ok(())
}

#[test]
fn sync_pull_then_run_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _store) = temp_store()?;
    let (remote, _remote_id) =
        seed_remote(dir.path(), "2026-05-13T08:00:00Z", "remote archive note")?;

    let local_store = dir.path().join("local.db");
    remote_set(&local_store, &remote)?;
    run_json(
        &local_store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T11:00:00Z",
            "note",
            "independent local note",
        ],
    )?;
    // Pull adopts the remote archive id, so the two sides can merge afterwards.
    run_json(&local_store, &["sync", "pull"])?;
    run_json(
        &local_store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T12:00:00Z",
            "note",
            "after pull note",
        ],
    )?;

    let synced = run_json(&local_store, &["sync", "run"])?;
    assert_eq!(synced["data"]["merged"], true);
    assert_eq!(synced["data"]["pushed"], true);
    Ok(())
}
