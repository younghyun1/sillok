//! Per-store sync configuration in `<store>.sync.json`.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::SillokError;
use crate::storage::path::with_suffix;

/// Sidecar schema written by 1.x.
pub const CONFIG_VERSION: u32 = 2;
pub const DEFAULT_BRANCH: &str = "main";
pub const DEFAULT_DIR: &str = "sillok";

/// Where the chronicle lives in the Git remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConfig {
    pub schema_version: u32,
    pub url: String,
    pub branch: String,
    /// Directory holding `manifest.json` and `events/`.
    pub dir: String,
    /// 0.10 single-file artifact; imported and deleted on the next sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_path: Option<String>,
}

impl SyncConfig {
    /// Builds and validates a configuration.
    pub fn new(
        url: String,
        branch: Option<String>,
        dir: Option<String>,
        legacy_path: Option<String>,
    ) -> Result<Self, SillokError> {
        let config = Self {
            schema_version: CONFIG_VERSION,
            url: url.trim().to_string(),
            branch: match branch {
                Some(value) => value.trim().to_string(),
                None => DEFAULT_BRANCH.to_string(),
            },
            dir: match dir {
                Some(value) => value.trim().trim_matches('/').to_string(),
                None => DEFAULT_DIR.to_string(),
            },
            legacy_path,
        };
        match config.validate() {
            Ok(()) => Ok(config),
            Err(error) => Err(error),
        }
    }

    /// Rejects values Git could read as options and paths that leave the worktree.
    pub fn validate(&self) -> Result<(), SillokError> {
        if self.url.is_empty() || self.url.starts_with('-') {
            return Err(SillokError::sync(
                "sync_config_error",
                "remote URL is empty or starts with `-`",
            ));
        }
        if self.branch.is_empty()
            || self.branch.starts_with('-')
            || self.branch.chars().any(|c| c.is_whitespace() || c == ':')
        {
            return Err(SillokError::sync(
                "sync_config_error",
                format!("invalid branch `{}`", self.branch),
            ));
        }
        if let Err(error) = relative_path(&self.dir) {
            return Err(error);
        }
        match &self.legacy_path {
            Some(path) => relative_path(path),
            None => Ok(()),
        }
    }
}

/// Sidecar path beside the store.
pub fn sidecar(store: &Path) -> PathBuf {
    with_suffix(store, ".sync.json")
}

/// Reads the sidecar, upgrading a 0.10 (schema 1) file in place.
pub fn read(store: &Path) -> Result<SyncConfig, SillokError> {
    let path = sidecar(store);
    let bytes = match std::fs::read(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(SillokError::sync(
                "sync_remote_missing",
                "no sync remote; run `sillok sync remote set <url>`",
            ));
        }
        Err(error) => return Err(error.into()),
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    match value.get("schema_version").and_then(Value::as_u64) {
        Some(1) => {
            let field = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_string);
            let url = match field("url") {
                Some(url) => url,
                None => {
                    return Err(SillokError::sync(
                        "sync_config_error",
                        "0.10 sidecar has no url",
                    ));
                }
            };
            let config = match SyncConfig::new(url, field("branch"), None, field("path")) {
                Ok(value) => value,
                Err(error) => return Err(error),
            };
            match write(store, &config) {
                Ok(_) => Ok(config),
                Err(error) => Err(error),
            }
        }
        _ => match serde_json::from_value::<SyncConfig>(value) {
            Ok(config) if config.schema_version == CONFIG_VERSION => match config.validate() {
                Ok(()) => Ok(config),
                Err(error) => Err(error),
            },
            Ok(config) => Err(SillokError::sync(
                "sync_config_error",
                format!(
                    "sync config schema {} needs a newer sillok",
                    config.schema_version
                ),
            )),
            Err(error) => Err(error.into()),
        },
    }
}

/// Writes the sidecar atomically.
pub fn write(store: &Path, config: &SyncConfig) -> Result<PathBuf, SillokError> {
    let path = sidecar(store);
    let temp = with_suffix(&path, ".tmp");
    let encoded = match serde_json::to_vec_pretty(config) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = crate::storage::path::ensure_parent(&path) {
        return Err(error);
    }
    if let Err(error) = std::fs::write(&temp, encoded) {
        return Err(error.into());
    }
    match std::fs::rename(&temp, &path) {
        Ok(()) => Ok(path),
        Err(error) => Err(error.into()),
    }
}

fn relative_path(raw: &str) -> Result<(), SillokError> {
    let path = Path::new(raw);
    let valid = !raw.is_empty()
        && !raw.starts_with('-')
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if valid {
        Ok(())
    } else {
        Err(SillokError::sync(
            "sync_config_error",
            format!("path `{raw}` must be relative and stay inside the repository"),
        ))
    }
}
