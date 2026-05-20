use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use tracing::error;

use crate::archive_codec::{LEGACY_ZSTD_LEVEL, decode_archive, encode_archive};
use crate::domain::archive::Archive;
use crate::domain::event::WorkContext;
use crate::domain::id::ChronicleId;
use crate::domain::time::Timestamp;
use crate::error::SillokError;

/// File-backed archive store with atomic writes and coarse locking.
#[derive(Debug, Clone)]
pub struct ArchiveStore {
    path: PathBuf,
}

impl ArchiveStore {
    /// Creates a store wrapper for a path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Returns the archive path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Initializes an archive if it does not already exist.
    pub fn init(
        &self,
        recorded_at: Timestamp,
        actor: String,
        context: WorkContext,
    ) -> Result<(Archive, bool), SillokError> {
        self.with_exclusive_lock(|store| {
            if store.path.exists() {
                let archive = store.read_existing_archive()?;
                Ok((archive, false))
            } else {
                let archive = Archive::new(recorded_at, actor, context);
                store.write_archive(&archive)?;
                Ok((archive, true))
            }
        })
    }

    /// Reads an existing archive. Missing stores return `None`.
    pub fn read_existing(&self) -> Result<Option<Archive>, SillokError> {
        self.with_shared_lock(|store| {
            if store.path.exists() {
                Ok(Some(store.read_existing_archive()?))
            } else {
                Ok(None)
            }
        })
    }

    /// Loads the archive or creates one in memory without writing it.
    pub fn read_or_new(
        &self,
        recorded_at: Timestamp,
        actor: String,
        context: WorkContext,
    ) -> Result<Archive, SillokError> {
        self.with_shared_lock(|store| {
            if store.path.exists() {
                store.read_existing_archive()
            } else {
                Ok(Archive::new(recorded_at, actor, context))
            }
        })
    }

    /// Performs one locked mutation and writes the resulting archive atomically.
    pub fn mutate<T, F>(
        &self,
        recorded_at: Timestamp,
        actor: String,
        context: WorkContext,
        f: F,
    ) -> Result<T, SillokError>
    where
        F: FnOnce(&mut Archive) -> Result<T, SillokError>,
    {
        self.with_exclusive_lock(|store| {
            let mut archive = if store.path.exists() {
                store.read_existing_archive()?
            } else {
                Archive::new(recorded_at, actor, context)
            };
            let result = f(&mut archive)?;
            store.write_archive(&archive)?;
            Ok(result)
        })
    }

    /// Backs up the current archive and replaces it with a fresh one.
    pub fn truncate(
        &self,
        recorded_at: Timestamp,
        actor: String,
        context: WorkContext,
    ) -> Result<Option<PathBuf>, SillokError> {
        self.with_exclusive_lock(|store| {
            let backup = if store.path.exists() {
                let backup_path = store.backup_path(recorded_at);
                fs::copy(&store.path, &backup_path)?;
                Some(backup_path)
            } else {
                None
            };
            let archive = Archive::new(recorded_at, actor, context);
            store.write_archive(&archive)?;
            Ok(backup)
        })
    }

    fn with_shared_lock<T, F>(&self, f: F) -> Result<T, SillokError>
    where
        F: FnOnce(&Self) -> Result<T, SillokError>,
    {
        self.ensure_parent_dir()?;
        let lock_path = self.lock_path();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        lock.lock_shared()?;
        let result = f(self);
        match lock.unlock() {
            Ok(()) => result,
            Err(error_value) => {
                error!(
                    lock_path = %lock_path.display(),
                    error = %error_value,
                    "Failed to release shared archive lock"
                );
                result
            }
        }
    }

    fn with_exclusive_lock<T, F>(&self, f: F) -> Result<T, SillokError>
    where
        F: FnOnce(&Self) -> Result<T, SillokError>,
    {
        self.ensure_parent_dir()?;
        let lock_path = self.lock_path();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        lock.lock_exclusive()?;
        let result = f(self);
        match lock.unlock() {
            Ok(()) => result,
            Err(error_value) => {
                error!(
                    lock_path = %lock_path.display(),
                    error = %error_value,
                    "Failed to release exclusive archive lock"
                );
                result
            }
        }
    }

    fn read_existing_archive(&self) -> Result<Archive, SillokError> {
        let mut file = File::open(&self.path)?;
        let mut compressed = Vec::new();
        file.read_to_end(&mut compressed)?;
        let archive = decode_archive(&compressed)?;
        Ok(archive)
    }

    fn write_archive(&self, archive: &Archive) -> Result<(), SillokError> {
        self.ensure_parent_dir()?;
        let compressed = encode_archive(archive, LEGACY_ZSTD_LEVEL)?;

        let temp_path = self.temp_path();
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            file.write_all(&compressed)?;
            file.sync_all()?;
        }
        drop(compressed);
        fs::rename(&temp_path, &self.path)?;
        self.sync_parent_dir()?;
        Ok(())
    }

    fn ensure_parent_dir(&self) -> Result<(), SillokError> {
        match self.path.parent() {
            Some(parent) => {
                fs::create_dir_all(parent)?;
                Ok(())
            }
            None => Err(SillokError::new(
                "store_path_error",
                format!("store path `{}` has no parent", self.path.display()),
            )),
        }
    }

    fn sync_parent_dir(&self) -> Result<(), SillokError> {
        #[cfg(unix)]
        {
            match self.path.parent() {
                Some(parent) => {
                    let dir = File::open(parent)?;
                    dir.sync_all()?;
                    Ok(())
                }
                None => Err(SillokError::new(
                    "store_path_error",
                    format!("store path `{}` has no parent", self.path.display()),
                )),
            }
        }
        #[cfg(not(unix))]
        {
            Ok(())
        }
    }

    fn lock_path(&self) -> PathBuf {
        let mut path = self.path.clone();
        path.set_extension("lock");
        path
    }

    fn temp_path(&self) -> PathBuf {
        let mut path = self.path.clone();
        path.set_extension(format!("{}.tmp", ChronicleId::new_v7()));
        path
    }

    fn backup_path(&self, timestamp: Timestamp) -> PathBuf {
        let mut path = self.path.clone();
        path.set_extension(format!("{}.bak.zst", timestamp.as_millis()));
        path
    }
}
