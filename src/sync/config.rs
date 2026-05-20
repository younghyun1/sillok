use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::id::ChronicleId;
use crate::error::SillokError;

pub const SYNC_CONFIG_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_SYNC_BRANCH: &str = "main";
pub const DEFAULT_SYNC_PATH: &str = "sillok.slk.zst";

/// Per-store sync configuration stored beside the live database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConfig {
    pub schema_version: u32,
    pub url: String,
    pub branch: String,
    pub path: String,
}

impl SyncConfig {
    /// Builds and validates a sync configuration.
    pub fn new(url: String, branch: String, path: String) -> Result<Self, SillokError> {
        let config = Self {
            schema_version: SYNC_CONFIG_SCHEMA_VERSION,
            url,
            branch,
            path,
        };
        config.validate()?;
        Ok(config)
    }

    /// Validates the config shape before any Git or filesystem access.
    pub fn validate(&self) -> Result<(), SillokError> {
        if self.schema_version != SYNC_CONFIG_SCHEMA_VERSION {
            return Err(SillokError::new(
                "sync_config_error",
                format!(
                    "sync config schema {} is not supported",
                    self.schema_version
                ),
            ));
        }
        if self.url.trim().is_empty() {
            return Err(SillokError::new("sync_config_error", "remote URL is empty"));
        }
        if self.branch.trim().is_empty() {
            return Err(SillokError::new(
                "sync_config_error",
                "remote branch is empty",
            ));
        }
        validate_artifact_path(&self.path)
    }
}

/// Returns the sidecar path for a store path.
pub fn sidecar_path(store_path: &Path) -> PathBuf {
    let mut sidecar = store_path.as_os_str().to_os_string();
    sidecar.push(".sync.json");
    PathBuf::from(sidecar)
}

/// Reads a sync config, returning `sync_remote_missing` when absent.
pub fn read_required(store_path: &Path) -> Result<SyncConfig, SillokError> {
    let path = sidecar_path(store_path);
    let bytes = match fs::read(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(SillokError::new(
                "sync_remote_missing",
                format!(
                    "sync remote is not configured for `{}`",
                    store_path.display()
                ),
            ));
        }
        Err(error) => return Err(error.into()),
    };
    let config = serde_json::from_slice::<SyncConfig>(&bytes)?;
    config.validate()?;
    Ok(config)
}

/// Persists a sync config beside the store using an atomic rename.
pub fn write(store_path: &Path, config: &SyncConfig) -> Result<PathBuf, SillokError> {
    config.validate()?;
    let path = sidecar_path(store_path);
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent)?,
        None => {
            return Err(SillokError::new(
                "store_path_error",
                format!("config path `{}` has no parent", path.display()),
            ));
        }
    }
    let mut temp_path = path.clone();
    temp_path.set_extension(format!("{}.sync.tmp", ChronicleId::new_v7()));
    let encoded = serde_json::to_vec_pretty(config)?;
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
    }
    fs::rename(&temp_path, &path)?;
    sync_parent_dir(&path)?;
    Ok(path)
}

fn validate_artifact_path(path: &str) -> Result<(), SillokError> {
    let parsed = Path::new(path);
    if path.trim().is_empty() || parsed.is_absolute() {
        return Err(SillokError::new(
            "sync_config_error",
            "artifact path must be a non-empty relative path",
        ));
    }
    for component in parsed.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(SillokError::new(
                    "sync_config_error",
                    format!("artifact path `{path}` must stay inside the Git worktree"),
                ));
            }
        }
    }
    Ok(())
}

fn sync_parent_dir(path: &Path) -> Result<(), SillokError> {
    #[cfg(unix)]
    {
        match path.parent() {
            Some(parent) => {
                let dir = File::open(parent)?;
                dir.sync_all()?;
                Ok(())
            }
            None => Err(SillokError::new(
                "store_path_error",
                format!("config path `{}` has no parent", path.display()),
            )),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
