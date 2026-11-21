//! Installing, inspecting, and refreshing the embedded skill.
//!
//! `install` is explicit and converges the target on this binary's skill,
//! replacing whatever is there. `refresh` runs unasked from `sillok status`,
//! so it only replaces a copy Sillok wrote and nobody edited, and never
//! fails the command it rides on.

use std::path::Path;

use crate::error::SillokError;
use crate::skill::content::{self, VERSION, Version};
use crate::skill::file;
use crate::skill::roots::{Homes, Target};

/// Disables the refresh done by `sillok status` when set to `0`, `false`, or `off`.
pub const AUTOUPDATE_ENV: &str = "SILLOK_SKILL_AUTOUPDATE";

/// Larger files are not skills Sillok wrote; they are left unread.
const MAX_SKILL_BYTES: u64 = 1 << 20;

/// What an explicit install did to one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// No file existed.
    Installed,
    /// A different file was replaced.
    Updated,
    /// The file already matched this binary.
    Unchanged,
}

impl Action {
    /// Stable label used in output.
    pub fn label(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
        }
    }
}

/// How an installed copy relates to this binary's skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// No file.
    Missing,
    /// Byte-identical to this binary's skill.
    Current,
    /// Written by an older or equal Sillok and not edited since.
    Outdated,
    /// Written by a newer Sillok and not edited since.
    Newer,
    /// Written by Sillok, then edited by hand.
    Modified,
    /// Not written by Sillok.
    Unmanaged,
}

impl State {
    /// Stable label used in output.
    pub fn label(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Current => "current",
            Self::Outdated => "outdated",
            Self::Newer => "newer",
            Self::Modified => "modified",
            Self::Unmanaged => "unmanaged",
        }
    }
}

/// State of the copy at `path` and the version stamped in it, if any.
pub fn state(path: &Path) -> Result<(State, Option<Version>), SillokError> {
    let existing = match read(path) {
        Ok(Some(text)) => text,
        Ok(None) => return Ok((State::Missing, None)),
        Err(error) => return Err(error),
    };
    if existing == content::render() {
        return Ok((State::Current, content::parse_version(VERSION)));
    }
    let stamp = match content::inspect(&existing) {
        Some(value) => value,
        None => return Ok((State::Unmanaged, None)),
    };
    let state = match (stamp.pristine, content::parse_version(VERSION)) {
        (false, _) => State::Modified,
        (true, Some(current)) if stamp.version > current => State::Newer,
        (true, _) => State::Outdated,
    };
    Ok((state, Some(stamp.version)))
}

/// Makes `path` hold this binary's skill, replacing any existing file.
pub fn install(path: &Path) -> Result<Action, SillokError> {
    let rendered = content::render();
    let action = match read(path) {
        Ok(Some(existing)) if existing == rendered => return Ok(Action::Unchanged),
        Ok(Some(_)) => Action::Updated,
        Ok(None) => Action::Installed,
        Err(error) => return Err(error),
    };
    match file::write_atomic(path, &rendered) {
        Ok(()) => Ok(action),
        Err(error) => Err(error),
    }
}

/// Replaces outdated, unedited copies for the tools on this machine and
/// returns notices for the caller's warnings. Never fails.
pub fn refresh() -> Vec<String> {
    match std::env::var(AUTOUPDATE_ENV) {
        Ok(value) if matches!(value.trim(), "0" | "false" | "off") => return Vec::new(),
        Ok(_) | Err(_) => {}
    }
    match Homes::from_env() {
        Some(homes) => refresh_targets(&homes.detected()),
        None => Vec::new(),
    }
}

/// `refresh` over explicit targets.
pub fn refresh_targets(targets: &[Target]) -> Vec<String> {
    let mut notices = Vec::new();
    for target in targets {
        let shown = target.path.display();
        match state(&target.path) {
            Ok((State::Outdated, _)) => match install(&target.path) {
                Ok(_) => notices.push(format!(
                    "refreshed the sillok skill at {shown} to {VERSION}; run `sillok guide` for the current workflow"
                )),
                Err(error) => notices.push(format!(
                    "could not refresh the sillok skill at {shown}: {error}"
                )),
            },
            Ok((State::Modified, Some(stamped)))
                if Some(stamped) < content::parse_version(VERSION) =>
            {
                notices.push(format!(
                    "the sillok skill at {shown} was edited and predates sillok {VERSION}; run `sillok skill install` to replace it"
                ));
            }
            Ok(_) => {}
            Err(error) => notices.push(format!(
                "could not check the sillok skill at {shown}: {error}"
            )),
        }
    }
    notices
}

/// Reads a skill file; `None` when it does not exist. An oversized file
/// reads as empty text, which classifies as unmanaged.
fn read(path: &Path) -> Result<Option<String>, SillokError> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_SKILL_BYTES => return Ok(Some(String::new())),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Action, State, install, refresh_targets, state};
    use crate::skill::content;
    use crate::skill::roots::dir_target;

    fn state_of(path: &Path) -> State {
        match state(path) {
            Ok((value, _)) => value,
            Err(error) => panic!("state of {}: {error}", path.display()),
        }
    }

    fn installed(path: &Path) -> Action {
        match install(path) {
            Ok(value) => value,
            Err(error) => panic!("install to {}: {error}", path.display()),
        }
    }

    fn write(path: &Path, text: &str) {
        let made = match path.parent() {
            Some(parent) => std::fs::create_dir_all(parent),
            None => Ok(()),
        };
        if let Err(error) = made.and(std::fs::write(path, text)) {
            panic!("write {}: {error}", path.display());
        }
    }

    fn read(path: &Path) -> String {
        match std::fs::read_to_string(path) {
            Ok(value) => value,
            Err(error) => panic!("read {}: {error}", path.display()),
        }
    }

    fn sandbox() -> tempfile::TempDir {
        match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => panic!("temporary directory: {error}"),
        }
    }

    #[test]
    fn install_is_idempotent_and_replaces_foreign_files() {
        let dir = sandbox();
        let target = dir_target(dir.path());
        assert_eq!(state_of(&target.path), State::Missing);
        assert_eq!(installed(&target.path), Action::Installed);
        assert_eq!(installed(&target.path), Action::Unchanged);
        assert_eq!(state_of(&target.path), State::Current);
        write(&target.path, "---\nname: sillok\n---\nHand written.\n");
        assert_eq!(state_of(&target.path), State::Unmanaged);
        assert_eq!(installed(&target.path), Action::Updated);
        assert_eq!(read(&target.path), content::render());
        // One file in the skill directory: no temporary or backup is left behind.
        let entries = match std::fs::read_dir(dir.path().join("sillok")) {
            Ok(value) => value.count(),
            Err(error) => panic!("list skill directory: {error}"),
        };
        assert_eq!(entries, 1);
    }

    #[test]
    fn refresh_replaces_only_unedited_older_copies() {
        let dir = sandbox();
        let stale = dir_target(&dir.path().join("stale"));
        let edited = dir_target(&dir.path().join("edited"));
        let newer = dir_target(&dir.path().join("newer"));
        let foreign = dir_target(&dir.path().join("foreign"));
        let absent = dir_target(&dir.path().join("absent"));
        let older = content::render_as("0.0.1");
        let hand_edit = older.replace("Record as you go", "Record whenever");
        let future = content::render_as("999.0.0");
        write(&stale.path, &older);
        write(&edited.path, &hand_edit);
        write(&newer.path, &future);
        write(&foreign.path, "# My own notes\n");
        assert_eq!(state_of(&stale.path), State::Outdated);
        assert_eq!(state_of(&edited.path), State::Modified);
        assert_eq!(state_of(&newer.path), State::Newer);
        let targets = [
            stale.clone(),
            edited.clone(),
            newer.clone(),
            foreign.clone(),
            absent.clone(),
        ];
        let notices = refresh_targets(&targets);
        assert_eq!(notices.len(), 2);
        assert!(notices[0].contains("refreshed"));
        assert!(notices[1].contains("was edited"));
        assert_eq!(read(&stale.path), content::render());
        assert_eq!(read(&edited.path), hand_edit);
        assert_eq!(read(&newer.path), future);
        assert_eq!(read(&foreign.path), "# My own notes\n");
        assert!(!absent.path.exists());
        // A second pass has nothing left to replace; the edited copy still warns.
        assert_eq!(refresh_targets(&targets).len(), 1);
    }
}
