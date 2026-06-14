use std::io::{IsTerminal, Write};

use crate::cli::output::OutputMode;
use crate::domain::archive::Archive;
use crate::domain::id::ChronicleId;
use crate::error::SillokError;
use crate::sync::merge::{MergeOutcome, merge_archives};

/// Direction of a single sync operation.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SyncMode {
    /// Overwrite the local store with the remote archive.
    Pull,
    /// Overwrite the remote archive with the local store.
    Push,
    /// Merge same-archive sides; prompt to overwrite when they differ.
    Run,
}

impl SyncMode {
    /// Stable action label used in the command response.
    pub(crate) fn action(self) -> &'static str {
        match self {
            Self::Pull => "pull",
            Self::Push => "push",
            Self::Run => "run",
        }
    }
}

/// Which side a sync attempt writes to, and whether an event merge happened.
pub(crate) struct Target {
    /// Archive the local database should reflect; `None` leaves it untouched.
    pub(crate) local_target: Option<Archive>,
    /// Archive to write to the remote; `None` skips the push.
    pub(crate) remote_target: Option<Archive>,
    pub(crate) merged: bool,
}

/// Side to keep when `run` cannot merge two different archives.
enum Keep {
    Local,
    Remote,
}

/// Picks which archive each side should adopt for the chosen sync mode.
///
/// `pull` and `push` overwrite wholesale and never consult `archive_id`. `run`
/// keeps the event-level merge when both sides share an `archive_id`, and asks
/// the user which side to keep otherwise.
pub(crate) fn select_target(
    mode: SyncMode,
    output_mode: OutputMode,
    local: Option<&Archive>,
    remote: Option<&Archive>,
) -> Result<Target, SillokError> {
    match mode {
        SyncMode::Pull => Ok(Target {
            local_target: remote.cloned(),
            remote_target: None,
            merged: false,
        }),
        SyncMode::Push => Ok(Target {
            local_target: None,
            remote_target: local.cloned(),
            merged: false,
        }),
        SyncMode::Run => match (local, remote) {
            (Some(local_archive), Some(remote_archive))
                if local_archive.archive_id != remote_archive.archive_id =>
            {
                match resolve_mismatch(
                    output_mode,
                    local_archive.archive_id,
                    remote_archive.archive_id,
                )? {
                    Keep::Local => Ok(Target {
                        local_target: None,
                        remote_target: Some(local_archive.clone()),
                        merged: false,
                    }),
                    Keep::Remote => Ok(Target {
                        local_target: Some(remote_archive.clone()),
                        remote_target: None,
                        merged: false,
                    }),
                }
            }
            _ => {
                let MergeOutcome { archive, merged } = merge_archives(local, remote)?;
                let Some(archive) = archive else {
                    return Err(SillokError::new(
                        "archive_missing",
                        "cannot sync because local and remote archives are both missing",
                    ));
                };
                Ok(Target {
                    local_target: Some(archive.clone()),
                    remote_target: Some(archive),
                    merged,
                })
            }
        },
    }
}

/// Asks the user which side to keep when `run` cannot merge two archives.
///
/// Interactive terminals are prompted; agent and `--json` runs refuse so they
/// never lose data silently.
fn resolve_mismatch(
    output_mode: OutputMode,
    local_id: ChronicleId,
    remote_id: ChronicleId,
) -> Result<Keep, SillokError> {
    let interactive = output_mode != OutputMode::Json && std::io::stdin().is_terminal();
    if !interactive {
        return Err(SillokError::new(
            "sync_mismatch_needs_choice",
            format!(
                "local archive `{local_id}` and remote archive `{remote_id}` differ; \
                 rerun `sync pull` to keep the remote or `sync push` to keep the local"
            ),
        ));
    }
    loop {
        eprint!(
            "local archive `{local_id}` does not match remote archive `{remote_id}`. \
             keep [l]ocal / [r]emote / [a]bort? "
        );
        std::io::stderr().flush().ok();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line)? == 0 {
            return Err(SillokError::new(
                "sync_aborted",
                "sync aborted at the mismatch prompt",
            ));
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "l" | "local" => return Ok(Keep::Local),
            "r" | "remote" => return Ok(Keep::Remote),
            "a" | "abort" => {
                return Err(SillokError::new("sync_aborted", "sync aborted by the user"));
            }
            _ => continue,
        }
    }
}
