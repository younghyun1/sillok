//! Replacing a configuration file without ever exposing half of it.

use std::path::Path;

use crate::error::SillokError;

/// Writes through a sibling temporary file and a rename, so a reader never
/// sees a partial file and a crash leaves the old one intact.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), SillokError> {
    let (dir, name) = match (path.parent(), path.file_name().and_then(|n| n.to_str())) {
        (Some(dir), Some(name)) => (dir, name),
        _ => {
            return Err(SillokError::invalid(
                "skill_path_error",
                format!("`{}` is not a file path", path.display()),
            ));
        }
    };
    if let Err(error) = std::fs::create_dir_all(dir) {
        return Err(error.into());
    }
    let temp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    if let Err(error) = std::fs::write(&temp, text) {
        return Err(error.into());
    }
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            // Best effort: the rename failure is the error worth reporting.
            match std::fs::remove_file(&temp) {
                Ok(()) | Err(_) => {}
            }
            Err(error.into())
        }
    }
}
