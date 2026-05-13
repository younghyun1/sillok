use std::path::PathBuf;

use directories::BaseDirs;

use crate::error::SillokError;

/// Resolves the default user-global v2 database path.
pub fn default_store_path() -> Result<PathBuf, SillokError> {
    match std::env::var("SILLOK_STORE") {
        Ok(value) if !value.trim().is_empty() => return Ok(PathBuf::from(value)),
        Ok(_) | Err(_) => {}
    }

    default_base_path("sillok.db")
}

/// Resolves the legacy v1 compressed archive path used before v0.5.0.
pub fn legacy_default_store_path() -> Result<PathBuf, SillokError> {
    default_base_path("sillok.slk.zst")
}

fn default_base_path(filename: &str) -> Result<PathBuf, SillokError> {
    match BaseDirs::new() {
        Some(base_dirs) => Ok(base_dirs.data_local_dir().join("sillok").join(filename)),
        None => match std::env::var("HOME") {
            Ok(home) => Ok(PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("sillok")
                .join(filename)),
            Err(error) => Err(SillokError::new(
                "store_path_error",
                format!("could not resolve home directory: {error}"),
            )),
        },
    }
}
