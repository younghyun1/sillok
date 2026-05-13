use std::path::{Path, PathBuf};

use crate::error::SillokError;
use crate::storage::sql::store::SqlStore;
use crate::storage::store::ArchiveStore;

/// User-selected store path plus backend detection helpers.
#[derive(Debug, Clone)]
pub struct StoreHandle {
    path: PathBuf,
}

impl StoreHandle {
    /// Creates a handle for a concrete store path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Returns the requested store path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns whether the path names a legacy compressed archive.
    pub fn is_legacy_path(&self) -> bool {
        self.path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.ends_with(".slk.zst"))
    }

    /// Builds a legacy archive store wrapper.
    pub fn legacy(&self) -> ArchiveStore {
        ArchiveStore::new(self.path.clone())
    }

    /// Builds a Turso-backed v2 store wrapper.
    pub fn sql(&self) -> SqlStore {
        SqlStore::new(self.path.clone())
    }

    /// Requires a mutable v2 database path.
    pub fn require_sql_mutation(&self) -> Result<SqlStore, SillokError> {
        if self.is_legacy_path() {
            Err(SillokError::new(
                "migration_required",
                format!(
                    "`{}` is a legacy archive; run `sillok migrate --store {}` first",
                    self.path.display(),
                    self.path.display()
                ),
            ))
        } else {
            Ok(self.sql())
        }
    }
}
