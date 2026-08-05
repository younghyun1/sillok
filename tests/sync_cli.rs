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
fn sync_seeds_remote_artifact() -> Result<(), Box<dyn std::error::Error>> {
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
            "sync seed",
        ],
    )?;

    // A bare `sync` defaults to `run`.
    let synced = run_json(&store, &["sync"])?;
    assert_eq!(synced["data"]["action"], "run");
    assert_eq!(synced["data"]["pushed"], true);
    assert!(synced["data"]["commit"].is_string());

    let checkout = dir.path().join("checkout");
    clone_remote(&remote, &checkout)?;
    assert!(checkout.join("sillok.slk.zst").exists());
    Ok(())
}

#[test]
fn sync_adopts_remote_when_local_missing() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _store) = temp_store()?;
    let (remote, remote_id) = seed_remote(dir.path(), "2026-05-13T09:00:00Z", "remote only note")?;

    let target_store = dir.path().join("target.db");
    remote_set(&target_store, &remote)?;
    let synced = run_json(&target_store, &["sync", "run"])?;
    assert_eq!(synced["data"]["pulled"], true);
    assert_eq!(synced["data"]["local_after"]["archive_id"], remote_id);
    assert!(target_store.exists());

    let day = run_json(
        &target_store,
        &["--tz", "UTC", "day", "--date", "2026-05-13"],
    )?;
    assert_eq!(day["data"]["records"][0]["text"], "remote only note");
    Ok(())
}

#[test]
fn sync_merges_diverged_archives() -> Result<(), Box<dyn std::error::Error>> {
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
    run_json(&left_store, &["sync", "run"])?;

    let right_store = dir.path().join("right.db");
    remote_set(&right_store, &remote)?;
    run_json(&right_store, &["sync", "run"])?;
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
    run_json(&left_store, &["sync", "run"])?;
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
    run_json(&verify_store, &["sync", "run"])?;
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
    let synced = run_json(&source, &["sync", "run"])?;
    let archive_id = synced["data"]["remote_after"]["archive_id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| boxed_error("missing source archive id".to_string()))?;
    Ok((remote, archive_id))
}

#[test]
fn sync_meshes_independent_archives() -> Result<(), Box<dyn std::error::Error>> {
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

    // The two stores never shared an archive_id; sync meshes them anyway.
    let synced = run_json(&local_store, &["sync", "run"])?;
    assert_eq!(synced["data"]["merged"], true);
    assert_eq!(synced["data"]["pulled"], true);
    assert_eq!(synced["data"]["pushed"], true);
    // The source store was initialized first, so its identity survives, and
    // the local rebuild keeps a backup of the pre-mesh database.
    assert_eq!(synced["data"]["local_after"]["archive_id"], remote_id);
    assert!(synced["data"]["backup"].is_string());

    // Both sides' notes land under one day record.
    let day = run_json(
        &local_store,
        &["--tz", "UTC", "day", "--date", "2026-05-13"],
    )?;
    assert_eq!(day["data"]["records"].as_array().map(Vec::len), Some(2));
    assert_eq!(day["data"]["records"][0]["text"], "remote archive note");
    assert_eq!(day["data"]["records"][1]["text"], "independent local note");

    // The source store converges on the same meshed archive without pushing.
    let source_store = dir.path().join("source.db");
    let converged = run_json(&source_store, &["sync", "run"])?;
    assert_eq!(converged["data"]["pulled"], true);
    assert_eq!(converged["data"]["pushed"], false);
    assert_eq!(converged["data"]["local_after"]["archive_id"], remote_id);
    let source_day = run_json(
        &source_store,
        &["--tz", "UTC", "day", "--date", "2026-05-13"],
    )?;
    assert_eq!(
        source_day["data"]["records"].as_array().map(Vec::len),
        Some(2)
    );
    Ok(())
}

#[test]
fn sync_errors_when_both_archives_missing() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _store) = temp_store()?;
    let remote = init_bare_remote(dir.path())?;
    let ghost_store = dir.path().join("ghost.db");
    remote_set(&ghost_store, &remote)?;

    let failed = run_failure_json(&ghost_store, &["sync", "run"])?;
    assert_eq!(failed["error"]["code"], "archive_missing");
    Ok(())
}
