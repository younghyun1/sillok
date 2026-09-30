//! Store and sidecar path resolution.

use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::error::SillokError;

/// Default store: `$XDG_DATA_HOME/sillok/sillok.db` (or the platform data dir).
///
/// The path is unchanged from 0.10, so upgrading migrates the existing store
/// in place on first use.
pub fn default_store_path() -> Result<PathBuf, SillokError> {
    match BaseDirs::new() {
        Some(dirs) => Ok(dirs.data_local_dir().join("sillok").join("sillok.db")),
        None => match std::env::var("HOME") {
            Ok(home) => Ok(PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("sillok")
                .join("sillok.db")),
            Err(error) => Err(SillokError::invalid(
                "store_path_error",
                format!("could not resolve a home directory: {error}"),
            )),
        },
    }
}

/// Appends a suffix to a path without touching its extension
/// (`sillok.db` + `.sync.json` -> `sillok.db.sync.json`).
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut raw = path.as_os_str().to_os_string();
    raw.push(suffix);
    PathBuf::from(raw)
}

/// Creates the parent directory of a store path.
pub fn ensure_parent(path: &Path) -> Result<(), SillokError> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => match std::fs::create_dir_all(parent) {
            Ok(()) => Ok(()),
            Err(error) => Err(error.into()),
        },
        Some(_) => Ok(()),
        None => Err(SillokError::invalid(
            "store_path_error",
            format!("store path `{}` has no parent", path.display()),
        )),
    }
}

/// Removes a file, treating "already gone" as success.
pub fn remove_if_exists(path: &Path) -> Result<(), SillokError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
