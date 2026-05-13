use std::path::PathBuf;

use directories::BaseDirs;

use crate::error::SillokError;

/// Resolves the default user-global archive path.
pub fn default_store_path() -> Result<PathBuf, SillokError> {
    match std::env::var("SILLOK_STORE") {
        Ok(value) if !value.trim().is_empty() => return Ok(PathBuf::from(value)),
        Ok(_) | Err(_) => {}
    }

    match BaseDirs::new() {
        Some(base_dirs) => Ok(base_dirs
            .data_local_dir()
            .join("sillok")
            .join("sillok.slk.zst")),
        None => match std::env::var("HOME") {
            Ok(home) => Ok(PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("sillok")
                .join("sillok.slk.zst")),
            Err(error) => Err(SillokError::new(
                "store_path_error",
                format!("could not resolve home directory: {error}"),
            )),
        },
    }
}
