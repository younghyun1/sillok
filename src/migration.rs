use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::cli::args::MigrateArgs;
use crate::cli::human;
use crate::cli::output::{CommandOutcome, OutputMode};
use crate::domain::view::ChronicleView;
use crate::error::SillokError;
use crate::operation::OperationContext;
use crate::storage::path::legacy_default_store_path;
use crate::storage::sql::schema::STORE_DATASHAPE_VERSION;
use crate::storage::sql::store::SqlStore;
use crate::storage::store::ArchiveStore;

/// Handles `migrate` from the v1 compressed archive to the v2 Turso store.
pub async fn migrate(
    store_path: Option<PathBuf>,
    at: Option<String>,
    tz: Option<String>,
    output_mode: OutputMode,
    args: MigrateArgs,
) -> Result<CommandOutcome, SillokError> {
    let ctx = OperationContext::new(store_path.clone(), at, tz, output_mode)?;
    let source = match store_path {
        Some(path) => path,
        None => legacy_default_store_path()?,
    };
    if !source
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.ends_with(".slk.zst"))
    {
        return Err(SillokError::new(
            "invalid_migration_source",
            format!(
                "migration source `{}` is not a legacy .slk.zst archive",
                source.display()
            ),
        ));
    }
    let target = match args.target {
        Some(path) => path,
        None => default_target_for_source(&source)?,
    };
    let archive = match ArchiveStore::new(source.clone()).read_existing()? {
        Some(value) => value,
        None => {
            return Err(SillokError::new(
                "archive_missing",
                format!("legacy archive `{}` does not exist", source.display()),
            ));
        }
    };
    let view = ChronicleView::build(&archive)?;
    if args.dry_run {
        return Ok(migration_outcome(
            &source,
            &target,
            None,
            archive.events.len(),
            view.records.len(),
            true,
            ctx.warnings,
        ));
    }
    if !args.yes {
        return Err(SillokError::new(
            "confirmation_required",
            "migrate requires --yes unless --dry-run is used",
        ));
    }
    if target.exists() {
        return Err(SillokError::new(
            "target_exists",
            format!("target store `{}` already exists", target.display()),
        ));
    }
    ensure_parent_dir(&target)?;
    let backup = backup_source(&source, ctx.recorded_at)?;
    let temp_target = temp_target_for(&target, ctx.recorded_at);
    let stats = SqlStore::new(temp_target.clone())
        .import_archive(&archive)
        .await?;
    SqlStore::new(temp_target.clone()).doctor().await?;
    fs::rename(&temp_target, &target)?;
    Ok(migration_outcome(
        &source,
        &target,
        Some(&backup),
        stats.event_count,
        stats.record_count,
        false,
        ctx.warnings,
    ))
}

fn migration_outcome(
    source: &Path,
    target: &Path,
    backup: Option<&Path>,
    event_count: usize,
    record_count: usize,
    dry_run: bool,
    warnings: Vec<String>,
) -> CommandOutcome {
    CommandOutcome::new(
        "migrate",
        json!({
            "source": source.display().to_string(),
            "target": target.display().to_string(),
            "backup": backup.map(|path| path.display().to_string()),
            "dry_run": dry_run,
            "store_datashape_version": STORE_DATASHAPE_VERSION,
            "event_count": event_count,
            "record_count": record_count,
        }),
    )
    .with_warnings(warnings)
    .with_human(human::migrate(
        source,
        target,
        backup,
        event_count,
        record_count,
        dry_run,
    ))
}

fn default_target_for_source(source: &Path) -> Result<PathBuf, SillokError> {
    match source.parent() {
        Some(parent) => Ok(parent.join("sillok.db")),
        None => Err(SillokError::new(
            "store_path_error",
            format!("source path `{}` has no parent", source.display()),
        )),
    }
}

fn temp_target_for(target: &Path, timestamp: crate::domain::time::Timestamp) -> PathBuf {
    let mut temp = target.to_path_buf();
    temp.set_extension(format!("{}.tmp.db", timestamp.as_millis()));
    temp
}

fn backup_source(
    source: &Path,
    timestamp: crate::domain::time::Timestamp,
) -> Result<PathBuf, SillokError> {
    let mut backup = source.to_path_buf();
    backup.set_extension(format!("migrated-{}.bak.zst", timestamp.as_millis()));
    fs::copy(source, &backup)?;
    Ok(backup)
}

fn ensure_parent_dir(path: &Path) -> Result<(), SillokError> {
    match path.parent() {
        Some(parent) => {
            fs::create_dir_all(parent)?;
            Ok(())
        }
        None => Err(SillokError::new(
            "store_path_error",
            format!("store path `{}` has no parent", path.display()),
        )),
    }
}
