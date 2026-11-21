//! Leftovers `doctor` reports and `doctor --prune` removes.
//!
//! Migration and `reset` each leave a full copy of the store beside it, and
//! a sync that was killed leaves its temporary clone behind. Neither is ever
//! read again by Sillok, and nothing else removes them.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::sync::git::WORKTREE_PREFIX;

/// A sync clone untouched for this long belongs to no running sync.
const STALE_SYNC_DIR: Duration = Duration::from_secs(3_600);

/// Backup files beside the store, with their sizes: `<name>.bak.db` files
/// that share the store's stem, as written by migration, `reset`, and 0.x.
pub fn backups(store: &Path) -> Vec<(PathBuf, u64)> {
    let (dir, stem) = match (store.parent(), store.file_stem().and_then(|s| s.to_str())) {
        (Some(dir), Some(stem)) => (dir, format!("{stem}.")),
        _ => return Vec::new(),
    };
    let entries = match std::fs::read_dir(dir) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let matches = match name.to_str() {
            Some(text) => text.starts_with(&stem) && text.ends_with(".bak.db"),
            None => false,
        };
        if !matches || path == store {
            continue;
        }
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            found.push((path, meta.len()));
        }
    }
    found.sort();
    found
}

/// Temporary sync clones old enough that no sync can still be using them.
pub fn stale_sync_dirs() -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(std::env::temp_dir()) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let now = SystemTime::now();
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let named = match entry.file_name().to_str() {
            Some(text) => text.starts_with(WORKTREE_PREFIX),
            None => false,
        };
        let old = match entry.metadata().and_then(|meta| meta.modified()) {
            Ok(modified) => match now.duration_since(modified) {
                Ok(age) => age >= STALE_SYNC_DIR,
                Err(_) => false,
            },
            Err(_) => false,
        };
        if named && old && entry.path().is_dir() {
            found.push(entry.path());
        }
    }
    found.sort();
    found
}

/// What `--prune` removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Pruned {
    pub backups: usize,
    pub bytes: u64,
    pub sync_dirs: usize,
}

/// Removes every backup and stale sync clone; failures are returned as
/// warnings so one locked file does not stop the rest.
pub fn prune(store: &Path) -> (Pruned, Vec<String>) {
    let mut pruned = Pruned::default();
    let mut warnings = Vec::new();
    for (path, bytes) in backups(store) {
        match std::fs::remove_file(&path) {
            Ok(()) => {
                pruned.backups += 1;
                pruned.bytes += bytes;
            }
            Err(error) => warnings.push(format!("could not remove {}: {error}", path.display())),
        }
    }
    for path in stale_sync_dirs() {
        match std::fs::remove_dir_all(&path) {
            Ok(()) => pruned.sync_dirs += 1,
            Err(error) => warnings.push(format!("could not remove {}: {error}", path.display())),
        }
    }
    (pruned, warnings)
}
