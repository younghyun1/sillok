use std::path::Path;

use serde::Serialize;
use serde_json::json;

use crate::cli::args::SyncRemoteSetArgs;
use crate::cli::output::{CommandOutcome, OutputMode};
use crate::domain::archive::Archive;
use crate::error::SillokError;
use crate::operation::OperationContext;
use crate::storage::sql::store::SqlStore;
use crate::sync::config::{
    DEFAULT_SYNC_BRANCH, DEFAULT_SYNC_PATH, SyncConfig, read_required, sidecar_path, write,
};
use crate::sync::git::{GitWorktree, PushOutcome};
use crate::sync::plan::{SyncMode, select_target};

/// Stores the Git remote configuration for this Sillok store.
pub async fn remote_set(
    ctx: OperationContext,
    args: SyncRemoteSetArgs,
) -> Result<CommandOutcome, SillokError> {
    reject_legacy_sync(ctx.store.path())?;
    let branch = args
        .branch
        .unwrap_or_else(|| DEFAULT_SYNC_BRANCH.to_string());
    let path = args.path.unwrap_or_else(|| DEFAULT_SYNC_PATH.to_string());
    let config = SyncConfig::new(args.url, branch, path)?;
    let sidecar = write(ctx.store.path(), &config)?;
    Ok(sync_outcome(
        "remote_set",
        &config,
        json!({
            "sidecar": sidecar.display().to_string(),
        }),
        ctx.warnings,
    ))
}

/// Shows the Git remote configuration for this Sillok store.
pub async fn remote_show(ctx: OperationContext) -> Result<CommandOutcome, SillokError> {
    reject_legacy_sync(ctx.store.path())?;
    let config = read_required(ctx.store.path())?;
    Ok(sync_outcome(
        "remote_show",
        &config,
        json!({
            "sidecar": sidecar_path(ctx.store.path()).display().to_string(),
        }),
        ctx.warnings,
    ))
}

/// Pulls the remote artifact and rebuilds local projections.
pub async fn pull(ctx: OperationContext) -> Result<CommandOutcome, SillokError> {
    sync_archive(ctx, SyncMode::Pull).await
}

/// Pushes the local archive artifact to the configured remote.
pub async fn push(ctx: OperationContext) -> Result<CommandOutcome, SillokError> {
    sync_archive(ctx, SyncMode::Push).await
}

/// Runs bidirectional sync with one push-rejection retry.
pub async fn run(ctx: OperationContext) -> Result<CommandOutcome, SillokError> {
    sync_archive(ctx, SyncMode::Run).await
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct ArchiveSummary {
    archive_id: String,
    created_at: crate::domain::time::Timestamp,
    event_count: usize,
}

#[derive(Debug, Serialize)]
struct RemoteSummary<'a> {
    url: &'a str,
    branch: &'a str,
    path: &'a str,
}

#[derive(Debug)]
struct SyncAttempt {
    local_before: Option<ArchiveSummary>,
    remote_before: Option<ArchiveSummary>,
    local_after: Option<ArchiveSummary>,
    remote_after: Option<ArchiveSummary>,
    merged: bool,
    pulled: bool,
    pushed: bool,
    backup: Option<String>,
    commit: Option<String>,
}

async fn sync_archive(
    ctx: OperationContext,
    mode: SyncMode,
) -> Result<CommandOutcome, SillokError> {
    reject_legacy_sync(ctx.store.path())?;
    let config = read_required(ctx.store.path())?;
    let store = ctx.store.sql();
    let mut attempt = run_attempt(&store, &config, ctx.recorded_at, mode, ctx.output_mode).await;
    // A rejected push means the remote advanced under us; rebuild on the new
    // head and retry once. Pull never pushes, so it never reaches this branch.
    if matches!(
        attempt.as_ref().map_err(SillokError::code),
        Err("sync_push_rejected")
    ) {
        attempt = run_attempt(&store, &config, ctx.recorded_at, mode, ctx.output_mode).await;
    }
    let result = attempt?;
    Ok(sync_outcome(
        mode.action(),
        &config,
        json!({
            "local_before": result.local_before,
            "remote_before": result.remote_before,
            "local_after": result.local_after,
            "remote_after": result.remote_after,
            "merged": result.merged,
            "pulled": result.pulled,
            "pushed": result.pushed,
            "backup": result.backup,
            "commit": result.commit,
        }),
        ctx.warnings,
    ))
}

async fn run_attempt(
    store: &SqlStore,
    config: &SyncConfig,
    recorded_at: crate::domain::time::Timestamp,
    mode: SyncMode,
    output_mode: OutputMode,
) -> Result<SyncAttempt, SillokError> {
    let local = store.export_archive().await?;
    let local_before = local.as_ref().map(summary);

    // Push has nothing to upload without a local archive; fail before the fetch.
    if matches!(mode, SyncMode::Push) && local.is_none() {
        return Err(SillokError::new(
            "archive_missing",
            "cannot push because the local archive does not exist",
        ));
    }

    let worktree = GitWorktree::prepare(config.clone())?;
    let remote = worktree.read_archive()?;
    let remote_before = remote.as_ref().map(summary);

    let target = select_target(mode, output_mode, local.as_ref(), remote.as_ref())?;

    let (local_after, pulled, backup) = match &target.local_target {
        Some(archive) => {
            let (sum, pulled, backup) =
                rebuild_local_if_needed(store, local.as_ref(), archive, recorded_at).await?;
            (Some(sum), pulled, backup)
        }
        None => (local_before.clone(), false, None),
    };

    let PushOutcome { commit, pushed } = match &target.remote_target {
        Some(archive) => worktree.write_commit_push(archive)?,
        None => PushOutcome {
            commit: None,
            pushed: false,
        },
    };
    let remote_after = match &target.remote_target {
        Some(archive) => Some(summary(archive)),
        None => remote_before.clone(),
    };

    Ok(SyncAttempt {
        local_before,
        remote_before,
        local_after,
        remote_after,
        merged: target.merged,
        pulled,
        pushed,
        backup,
        commit,
    })
}

async fn rebuild_local_if_needed(
    store: &SqlStore,
    local: Option<&Archive>,
    merged: &Archive,
    recorded_at: crate::domain::time::Timestamp,
) -> Result<(ArchiveSummary, bool, Option<String>), SillokError> {
    if local.is_some_and(|archive| archive == merged) {
        return Ok((summary(merged), false, None));
    }
    let (_stats, backup) = store.rebuild_from_archive(merged, recorded_at).await?;
    Ok((
        summary(merged),
        true,
        backup.map(|path| path.display().to_string()),
    ))
}

fn summary(archive: &Archive) -> ArchiveSummary {
    ArchiveSummary {
        archive_id: archive.archive_id.to_string(),
        created_at: archive.created_at,
        event_count: archive.events.len(),
    }
}

fn sync_outcome(
    action: &'static str,
    config: &SyncConfig,
    extra: serde_json::Value,
    warnings: Vec<String>,
) -> CommandOutcome {
    let mut data = json!({
        "action": action,
        "remote": RemoteSummary {
            url: &config.url,
            branch: &config.branch,
            path: &config.path,
        },
    });
    merge_json(&mut data, extra);
    CommandOutcome::new("sync", data).with_warnings(warnings)
}

fn merge_json(target: &mut serde_json::Value, extra: serde_json::Value) {
    let (Some(target_object), Some(extra_object)) = (target.as_object_mut(), extra.as_object())
    else {
        return;
    };
    for (key, value) in extra_object {
        target_object.insert(key.clone(), value.clone());
    }
}

fn reject_legacy_sync(path: &Path) -> Result<(), SillokError> {
    if path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.ends_with(".slk.zst"))
    {
        Err(SillokError::new(
            "migration_required",
            "`sync` requires a v2 SQLite/Turso store",
        ))
    } else {
        Ok(())
    }
}
