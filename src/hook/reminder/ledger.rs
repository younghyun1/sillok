//! Per-session, append-only record of one turn's changes and Sillok writes.
//!
//! PostToolUse hooks for parallel tool calls run concurrently, so each call appends
//! at most two short lines in a single write. Stop reads and removes the file, which
//! starts the next turn empty. The default location is the temporary directory, so a
//! session that ends without a Stop leaves nothing beyond the host's temp cleanup.

use super::{classify::Activity, error::Error};
use std::{
    fs::{File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

/// Change lines stop once a turn's ledger reaches this size: the turn already owes a
/// record, and more descriptions would not be shown.
const CHANGE_LIMIT: u64 = 16 * 1024;
/// Nothing is appended past this size, which bounds the file however long a turn runs.
const LEDGER_LIMIT: u64 = 2 * CHANGE_LIMIT;
/// Distinct change descriptions quoted back in a reminder.
const SHOWN: usize = 3;
/// Characters kept from one change description.
const LABEL_CHARS: usize = 96;

/// Ledger directory: `SILLOK_REMINDER_DIR` when set, else `sillok-reminder` under the
/// per-user temporary directory.
pub fn directory() -> PathBuf {
    match std::env::var_os("SILLOK_REMINDER_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        Some(_) | None => std::env::temp_dir().join("sillok-reminder"),
    }
}

/// Append what one tool call did; calls that did neither are not written.
pub fn note(directory: &Path, session: &str, activity: &Activity) -> Result<(), Error> {
    let mut lines = String::new();
    if let Some(label) = &activity.change {
        lines.push_str("change\t");
        lines.extend(
            label
                .chars()
                .take(LABEL_CHARS)
                .map(|c| if c.is_control() { ' ' } else { c }),
        );
        lines.push('\n');
    }
    if activity.record {
        lines.push_str("record\n");
    }
    if lines.is_empty() {
        return Ok(());
    }
    match create(directory) {
        Ok(()) => (),
        Err(error) => return Err(Error::Io(error)),
    }
    let path = directory.join(session);
    let size = match std::fs::metadata(&path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == ErrorKind::NotFound => 0,
        Err(error) => return Err(Error::Io(error)),
    };
    if size >= LEDGER_LIMIT || (size >= CHANGE_LIMIT && !activity.record) {
        return Ok(());
    }
    let mut file = match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => file,
        Err(error) => return Err(Error::Io(error)),
    };
    // One write per call keeps concurrent appends from interleaving within a line.
    match file.write_all(lines.as_bytes()) {
        Ok(()) => Ok(()),
        Err(error) => Err(Error::Io(error)),
    }
}

/// Owner-only on Unix: the ledger names files and commands from the session.
fn create(directory: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(directory)
}

/// What a turn did, read back at Stop.
#[derive(Debug, Default, PartialEq)]
pub struct Turn {
    /// Up to three distinct change descriptions, in order.
    pub changes: Vec<String>,
    pub recorded: bool,
}

impl Turn {
    /// Continuation text when the turn changed something and recorded nothing.
    pub fn reminder(&self) -> Option<String> {
        if self.recorded || self.changes.is_empty() {
            return None;
        }
        Some(format!(
            "Sillok: this turn changed files or systems ({}) and recorded nothing. Record the completed work, anything still in flight (`--status active`), and any correction you made, each under its objective (`sillok status` lists open ones); then finish. If none of it is substantive, finish without recording.",
            self.changes.join("; ")
        ))
    }
}

/// Read and remove the session's ledger; a missing ledger is an empty turn.
pub fn take(directory: &Path, session: &str) -> Result<Turn, Error> {
    let path = directory.join(session);
    let mut bytes = Vec::new();
    match File::open(&path) {
        // The handle closes before removal, which Windows requires.
        Ok(file) => match file.take(LEDGER_LIMIT + 1024).read_to_end(&mut bytes) {
            Ok(_) => (),
            Err(error) => return Err(Error::Io(error)),
        },
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Turn::default()),
        Err(error) => return Err(Error::Io(error)),
    }
    match std::fs::remove_file(&path) {
        Ok(()) => (),
        Err(error) if error.kind() == ErrorKind::NotFound => (),
        Err(error) => return Err(Error::Io(error)),
    }
    Ok(parse(&String::from_utf8_lossy(&bytes)))
}

fn parse(text: &str) -> Turn {
    let mut turn = Turn::default();
    for line in text.lines() {
        match line.split_once('\t') {
            Some(("change", label)) => {
                if turn.changes.len() < SHOWN && !turn.changes.iter().any(|seen| seen == label) {
                    turn.changes.push(label.to_owned());
                }
            }
            Some(_) => (),
            None => turn.recorded |= line == "record",
        }
    }
    turn
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(label: &str) -> Activity {
        Activity {
            change: Some(label.to_owned()),
            record: false,
        }
    }

    #[test]
    fn round_trips_and_empties_the_turn() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        note(dir.path(), "s1", &change("git push"))?;
        note(dir.path(), "s1", &change("git push"))?;
        note(dir.path(), "s1", &change("edit\ta\nb"))?;
        note(dir.path(), "s1", &Activity::default())?;
        let turn = take(dir.path(), "s1")?;
        assert_eq!(
            turn.changes,
            vec!["git push".to_owned(), "edit a b".to_owned()]
        );
        assert!(!turn.recorded);
        assert!(
            turn.reminder()
                .is_some_and(|text| text.contains("git push; edit a b"))
        );
        assert_eq!(take(dir.path(), "s1")?, Turn::default());
        Ok(())
    }

    #[test]
    fn a_record_anywhere_in_the_turn_satisfies_it() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        note(dir.path(), "s2", &change("gh pr merge"))?;
        note(
            dir.path(),
            "s2",
            &Activity {
                change: None,
                record: true,
            },
        )?;
        let turn = take(dir.path(), "s2")?;
        assert!(turn.recorded);
        assert_eq!(turn.reminder(), None);
        Ok(())
    }

    #[test]
    fn ledger_size_is_bounded() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let long = "x".repeat(LABEL_CHARS * 4);
        for _ in 0..1_000 {
            note(dir.path(), "s3", &change(&long))?;
        }
        let size = std::fs::metadata(dir.path().join("s3"))?.len();
        assert!(size < CHANGE_LIMIT + 256);
        for _ in 0..10_000 {
            note(
                dir.path(),
                "s3",
                &Activity {
                    change: None,
                    record: true,
                },
            )?;
        }
        let size = std::fs::metadata(dir.path().join("s3"))?.len();
        assert!(size < LEDGER_LIMIT + 256);
        let turn = take(dir.path(), "s3")?;
        assert_eq!(turn.changes, vec!["x".repeat(LABEL_CHARS)]);
        assert!(turn.recorded);
        Ok(())
    }
}
