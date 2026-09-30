//! In-place migration of a 0.9/0.10 store to store version 3.
//!
//! Runs on the first open after an upgrade. A file lock makes concurrent
//! agents wait while one of them migrates; the others then open the new
//! store. The old database is kept as a single-file `VACUUM INTO` backup,
//! and the new database is built beside it and renamed into place only
//! after it validates, so a crash at any step leaves a usable store.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{TransactionBehavior, params};

use crate::domain::time::Timestamp;
use crate::error::SillokError;
use crate::legacy::convert::convert;
use crate::legacy::v2_store::{is_v2_store, read_v2_store};
use crate::storage::lock::FileLock;
use crate::storage::path::{remove_if_exists, with_suffix};
use crate::storage::sqlite::open::{StoreInfo, connect, write_info};
use crate::storage::sqlite::schema::{self, STORE_VERSION};
use crate::storage::sqlite::{events, rebuild};

/// Migrating 50k events takes well under this; the rest is headroom for slow disks.
const MIGRATION_WAIT: Duration = Duration::from_secs(120);

/// What one migration did.
#[derive(Debug, Clone)]
pub struct MigrationReport {
    pub legacy_events: usize,
    pub events: usize,
    pub dropped: usize,
    pub records: usize,
    pub backup: PathBuf,
}

impl MigrationReport {
    /// One-line notice for command output.
    pub fn summary(&self) -> String {
        format!(
            "migrated 0.10 store: {} events kept, {} day/archive markers dropped, {} records; backup at {}",
            self.events,
            self.dropped,
            self.records,
            self.backup.display()
        )
    }
}

/// Migrates `path` if it still holds a v2 store; `None` if another process
/// finished first.
pub fn migrate_v2(path: &Path) -> Result<Option<MigrationReport>, SillokError> {
    let lock = match FileLock::acquire(&with_suffix(path, ".migrate.lock"), MIGRATION_WAIT) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let old = match connect(path, false) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    match is_v2_store(&old) {
        Ok(true) => {}
        Ok(false) => return Ok(None),
        Err(error) => return Err(error),
    }
    let archive = match read_v2_store(&old) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let conversion = match convert(&archive) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let stamp = Timestamp::now().as_millis();
    let temp = with_suffix(path, &format!(".v3-{stamp}.tmp"));
    let backup = with_suffix(path, &format!(".v2-{stamp}.bak.db"));
    let records = match build(&temp, &conversion) {
        Ok(value) => value,
        Err(error) => {
            if let Err(cleanup) = remove_db(&temp) {
                tracing::warn!(path = %temp.display(), error = %cleanup, "Failed to remove partial migration");
            }
            return Err(error);
        }
    };
    // VACUUM INTO reads through the WAL, so the backup is complete and
    // self-contained even when 0.10 left events un-checkpointed.
    if let Err(error) = old.execute("VACUUM INTO ?1", params![backup.display().to_string()]) {
        return Err(error.into());
    }
    drop(old);
    // Old WAL/SHM files would be replayed onto the new database; the backup
    // already holds their contents.
    for suffix in ["-wal", "-shm"] {
        if let Err(error) = remove_if_exists(&with_suffix(path, suffix)) {
            return Err(error);
        }
    }
    if let Err(error) = std::fs::rename(&temp, path) {
        return Err(error.into());
    }
    tracing::info!(
        store = %path.display(),
        backup = %backup.display(),
        events = conversion.events.len(),
        "Migrated 0.10 store"
    );
    // Waiters holding the old inode re-check the version after locking, so
    // unlinking before release cannot cause a second migration.
    if let Err(error) = remove_if_exists(lock.path()) {
        tracing::warn!(path = %lock.path().display(), error = %error, "Failed to remove migration lock");
    }
    drop(lock);
    Ok(Some(MigrationReport {
        legacy_events: archive.events.len(),
        events: conversion.events.len(),
        dropped: conversion.dropped,
        records,
        backup,
    }))
}

/// Writes a complete v3 store to `temp` and returns its record count.
fn build(
    temp: &Path,
    conversion: &crate::legacy::convert::Conversion,
) -> Result<usize, SillokError> {
    if let Err(error) = remove_db(temp) {
        return Err(error);
    }
    let mut conn = match connect(temp, true) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
        row.get::<_, String>(0)
    }) {
        return Err(error.into());
    }
    let tx = match conn.transaction_with_behavior(TransactionBehavior::Immediate) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = schema::create(&tx) {
        return Err(error);
    }
    if let Err(error) = tx.execute(
        "INSERT INTO store_meta (store_meta_key, store_meta_value) VALUES ('store_version', ?1)",
        [STORE_VERSION],
    ) {
        return Err(error.into());
    }
    let info = StoreInfo {
        archive_id: conversion.archive_id,
        created_at: conversion.created_at,
    };
    if let Err(error) = write_info(&tx, info) {
        return Err(error);
    }
    for event in &conversion.events {
        if let Err(error) = events::insert(&tx, event) {
            return Err(error);
        }
    }
    let summary = match rebuild::rebuild(&tx) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = tx.commit() {
        return Err(error.into());
    }
    // Fold the WAL into the main file so the rename moves everything.
    if let Err(error) = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())) {
        return Err(error.into());
    }
    drop(conn);
    for suffix in ["-wal", "-shm"] {
        if let Err(error) = remove_if_exists(&with_suffix(temp, suffix)) {
            return Err(error);
        }
    }
    Ok(summary.records)
}

fn remove_db(path: &Path) -> Result<(), SillokError> {
    for suffix in ["", "-wal", "-shm"] {
        if let Err(error) = remove_if_exists(&with_suffix(path, suffix)) {
            return Err(error);
        }
    }
    Ok(())
}
