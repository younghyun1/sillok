//! Advisory file lock with a bounded wait.
//!
//! SQLite's WAL mode already serializes writers and lets readers run
//! concurrently, so normal commands take no lock of their own. This lock only
//! guards one-shot operations that replace the database file (migration from
//! 0.10), where two agents starting at once must not both migrate.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::SillokError;
use crate::storage::path::ensure_parent;

/// First retry delay; doubles up to `MAX_BACKOFF`.
const INITIAL_BACKOFF: Duration = Duration::from_millis(2);
const MAX_BACKOFF: Duration = Duration::from_millis(100);

/// Held exclusive lock; released when dropped (closing the file unlocks it).
#[derive(Debug)]
pub struct FileLock {
    _file: File,
    path: PathBuf,
}

impl FileLock {
    /// Acquires an exclusive lock on `path`, waiting at most `timeout`.
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self, SillokError> {
        if let Err(error) = ensure_parent(path) {
            return Err(error);
        }
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
        {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let deadline = Instant::now() + timeout;
        let mut backoff = INITIAL_BACKOFF;
        loop {
            match file.try_lock() {
                Ok(()) => {
                    return Ok(Self {
                        _file: file,
                        path: path.to_path_buf(),
                    });
                }
                Err(TryLockError::WouldBlock) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Err(SillokError::Busy(format!(
                            "`{}` stayed locked for {} ms",
                            path.display(),
                            timeout.as_millis()
                        )));
                    }
                    std::thread::sleep(backoff.min(deadline - now));
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
                Err(TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }

    /// Path of the lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::FileLock;
    use crate::error::SillokError;

    #[test]
    fn second_holder_times_out() -> Result<(), SillokError> {
        let dir = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let path = dir.path().join("x.lock");
        let first = match FileLock::acquire(&path, Duration::from_millis(50)) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let second = FileLock::acquire(&path, Duration::from_millis(30));
        assert!(matches!(second, Err(error) if error.code() == "store_busy"));
        drop(first);
        assert!(FileLock::acquire(&path, Duration::from_millis(50)).is_ok());
        Ok(())
    }
}
