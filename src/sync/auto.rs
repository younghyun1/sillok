//! Background sync started by `status`.
//!
//! Sync only helps when it runs, and nothing ran it: a store with a remote
//! could sit hundreds of events behind its other replica. `status` is the
//! command every session starts with, so when a remote is configured and the
//! last attempt is older than the interval, it starts `sillok sync` as a
//! detached child and returns at once. The child writes its result beside
//! the store, and the next `status` reports a failure instead of hiding it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::domain::time::Timestamp;
use crate::error::SillokError;
use crate::storage::path::with_suffix;
use crate::sync::config;
use crate::sync::exchange::Report;

/// Set to `0`, `false`, or `off` to stop `status` from starting a sync.
pub const AUTOSYNC_ENV: &str = "SILLOK_AUTOSYNC";

/// Minutes between background syncs when the sidecar does not say.
pub const DEFAULT_MINUTES: u32 = 60;

/// Longest failure text kept in the state file and shown by `status`.
const MAX_ERROR_CHARS: usize = 300;

/// Outcome of background syncs, in `<store>.sync.state.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// When the latest background sync was started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempted_at: Option<Timestamp>,
    /// When a background sync last finished. Older than `attempted_at` while
    /// one is running, or when one was killed before it could report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<Timestamp>,
    /// Whether that sync succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pulled: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed: Option<usize>,
    /// `code: message` of the failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// State file beside the store.
pub fn state_path(store: &Path) -> PathBuf {
    with_suffix(store, ".sync.state.json")
}

/// Reads the state; a missing or unreadable file is an empty state.
pub fn read_state(store: &Path) -> State {
    match std::fs::read(state_path(store)) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(state) => state,
            Err(_) => State::default(),
        },
        Err(_) => State::default(),
    }
}

/// Starts a background sync when one is due. Returns a notice when the last
/// one failed or a new one could not be started. Never fails the caller.
pub fn maybe_start(store: &Path, now: Timestamp) -> Option<String> {
    match std::env::var(AUTOSYNC_ENV) {
        Ok(value) if matches!(value.trim(), "0" | "false" | "off") => return None,
        Ok(_) | Err(_) => {}
    }
    // No remote, or a sidecar this build cannot use: nothing to do here, and
    // `sillok sync` is where a broken sidecar is reported.
    let minutes = match config::read(store) {
        Ok(config) => match config.auto_minutes {
            Some(value) => value,
            None => DEFAULT_MINUTES,
        },
        Err(_) => return None,
    };
    if minutes == 0 {
        return None;
    }
    let mut state = read_state(store);
    let failure = match (&state.ok, &state.error) {
        (Some(false), Some(error)) => Some(format!(
            "the last background sync failed ({error}); run `sillok sync` to retry"
        )),
        _ => None,
    };
    let due = match state.attempted_at {
        Some(at) => {
            let elapsed = now.as_millis() - at.as_millis();
            // A negative gap means the clock moved back; do not wait it out.
            elapsed < 0 || elapsed >= i64::from(minutes) * 60_000
        }
        None => true,
    };
    if !due {
        return failure;
    }
    // Stamp the attempt before starting it, so sessions that open together
    // start one sync, and a failure to stamp cannot start one per command.
    state.attempted_at = Some(now);
    if write_state(store, &state).is_err() {
        return failure;
    }
    match spawn(store) {
        Ok(()) => failure,
        Err(error) => Some(format!("could not start a background sync: {error}")),
    }
}

/// Records how a background sync ended. Called by the child.
pub fn finish(store: &Path, result: Result<&Report, &SillokError>) {
    let mut state = read_state(store);
    state.finished_at = Some(Timestamp::now());
    match result {
        Ok(report) => {
            state.ok = Some(true);
            state.pulled = Some(report.pulled);
            state.pushed = Some(report.pushed);
            state.error = None;
        }
        Err(error) => {
            state.ok = Some(false);
            state.pulled = None;
            state.pushed = None;
            let text = format!("{}: {error}", error.code());
            state.error = Some(text.chars().take(MAX_ERROR_CHARS).collect());
        }
    }
    // Nowhere to report a failure to write: the child's output is discarded.
    match write_state(store, &state) {
        Ok(()) | Err(_) => {}
    }
}

fn write_state(store: &Path, state: &State) -> Result<(), SillokError> {
    let path = state_path(store);
    let temp = with_suffix(&path, &format!(".{}.tmp", std::process::id()));
    let encoded = match serde_json::to_vec(state) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = std::fs::write(&temp, encoded) {
        return Err(error.into());
    }
    match std::fs::rename(&temp, &path) {
        Ok(()) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Starts `sillok sync --background` detached from this process: its own
/// process group, no inherited streams, and the store's directory as its
/// working directory so it does not pin the caller's.
fn spawn(store: &Path) -> std::io::Result<()> {
    let exe = match std::env::current_exe() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut command = Command::new(exe);
    command
        .arg("--store")
        .arg(store)
        .args(["sync", "--background"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(parent) = store.parent()
        && parent.is_dir()
    {
        command.current_dir(parent);
    }
    detach(&mut command);
    // The child outlives this process on purpose and is never waited on;
    // the operating system reaps it once this process has exited.
    match command.spawn() {
        Ok(_child) => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    command.creation_flags(0x0000_0008 | 0x0000_0200);
}

#[cfg(test)]
mod tests {
    use super::{State, finish, maybe_start, read_state, state_path};
    use crate::domain::time::Timestamp;
    use crate::error::SillokError;
    use crate::sync::exchange::Report;

    #[test]
    fn nothing_starts_without_a_remote() {
        let dir = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => panic!("temporary directory: {error}"),
        };
        let store = dir.path().join("sillok.db");
        assert_eq!(maybe_start(&store, Timestamp::now()), None);
        assert!(!state_path(&store).exists());
    }

    #[test]
    fn results_round_trip_and_failures_are_bounded() {
        let dir = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => panic!("temporary directory: {error}"),
        };
        let store = dir.path().join("sillok.db");
        assert_eq!(read_state(&store), State::default());
        let report = Report {
            pulled: 3,
            pushed: 2,
            ..Report::default()
        };
        finish(&store, Ok(&report));
        let done = read_state(&store);
        assert_eq!(
            (done.ok, done.pulled, done.pushed),
            (Some(true), Some(3), Some(2))
        );
        let error = SillokError::sync("sync_git_error", "x".repeat(1_000));
        finish(&store, Err(&error));
        let failed = read_state(&store);
        assert_eq!(failed.ok, Some(false));
        assert_eq!(failed.pulled, None);
        match failed.error {
            Some(text) => {
                assert!(text.starts_with("sync_git_error: "));
                assert_eq!(text.chars().count(), 300);
            }
            None => panic!("a failure keeps its error"),
        }
    }
}
