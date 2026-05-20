use std::path::PathBuf;

mod sync_support;

use sync_support::{
    clone_remote, init_bare_remote, remote_set, run_failure_json, run_json, temp_store,
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

#[test]
fn sync_archive_mismatch_preserves_local_db() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, source_store) = temp_store()?;
    let remote = init_bare_remote(dir.path())?;
    remote_set(&source_store, &remote)?;
    run_json(
        &source_store,
        &[
            "--tz",
            "UTC",
            "--at",
            "2026-05-13T08:00:00Z",
            "note",
            "remote archive note",
        ],
    )?;
    run_json(&source_store, &["sync", "push"])?;

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

    let failed = run_failure_json(&local_store, &["sync", "run"])?;
    assert_eq!(failed["error"]["code"], "sync_archive_mismatch");
    let day = run_json(
        &local_store,
        &["--tz", "UTC", "day", "--date", "2026-05-13"],
    )?;
    assert_eq!(day["data"]["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(day["data"]["records"][0]["text"], "independent local note");
    Ok(())
}
