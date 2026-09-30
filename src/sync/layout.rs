//! The Git-side archive layout.
//!
//! ```text
//! <dir>/manifest.json          format, reader requirement, archive identity
//! <dir>/events/YYYY-MM.jsonl   one canonical event per line, UTC month of recorded_at
//! ```
//!
//! Lines are sorted by `(recorded_at, event_id)`, so new events append to
//! the current month's file and a sync commit is a small, readable diff.
//! Files are plain text; Git's packfiles compress and delta them.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::event::envelope::{EventKey, MAX_EVENT_BYTES};
use crate::domain::id::{ArchiveId, EventId};
use crate::domain::time::Timestamp;
use crate::error::SillokError;

pub const MANIFEST_FILE: &str = "manifest.json";
pub const EVENTS_DIR: &str = "events";
pub const FORMAT: &str = "sillok-archive";
/// Layout version this build writes.
pub const FORMAT_VERSION: u32 = 1;
/// Highest `min_reader_version` this build can read.
pub const READER_VERSION: u32 = 1;
/// Cap on bytes read from a remote, against runaway or hostile repositories.
pub const MAX_REMOTE_BYTES: u64 = 1 << 30;

/// `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    /// Readers older than this must refuse the archive.
    pub min_reader_version: u32,
    pub archive_id: ArchiveId,
    pub created_at: Timestamp,
}

impl Manifest {
    /// Manifest this build writes.
    pub fn new(archive_id: ArchiveId, created_at: Timestamp) -> Self {
        Self {
            format: FORMAT.to_string(),
            format_version: FORMAT_VERSION,
            min_reader_version: 1,
            archive_id,
            created_at,
        }
    }
}

/// One event line from the remote.
#[derive(Debug, Clone)]
pub struct Line {
    pub event_id: EventId,
    pub recorded_at: Timestamp,
    pub raw: String,
}

/// Everything read from a layout directory.
#[derive(Debug, Default)]
pub struct Layout {
    pub manifest: Option<Manifest>,
    pub lines: Vec<Line>,
}

/// Reads a layout; a missing directory is an empty layout.
pub fn read(dir: &Path) -> Result<Layout, SillokError> {
    let mut layout = Layout::default();
    let manifest_path = dir.join(MANIFEST_FILE);
    if manifest_path.exists() {
        let text = match read_capped(&manifest_path, 1 << 20) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let manifest: Manifest = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        if manifest.format != FORMAT || manifest.min_reader_version > READER_VERSION {
            return Err(SillokError::datashape(
                "unsupported_format",
                format!(
                    "remote archive needs reader version {}; this sillok reads {READER_VERSION}. Upgrade sillok.",
                    manifest.min_reader_version
                ),
            ));
        }
        layout.manifest = Some(manifest);
    }
    let events_dir = dir.join(EVENTS_DIR);
    let entries = match std::fs::read_dir(&events_dir) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(layout),
        Err(error) => return Err(error.into()),
    };
    let mut files = Vec::new();
    for entry in entries {
        match entry {
            Ok(value) => {
                let path = value.path();
                if path.extension().is_some_and(|ext| ext == "jsonl") {
                    files.push(path);
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    files.sort();
    let mut budget = MAX_REMOTE_BYTES;
    for file in files {
        let text = match read_capped(&file, budget) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        budget = budget.saturating_sub(text.len() as u64);
        for raw in text.lines().filter(|line| !line.trim().is_empty()) {
            if raw.len() > MAX_EVENT_BYTES {
                return Err(SillokError::datashape(
                    "invalid_event",
                    "remote event line exceeds 1 MiB",
                ));
            }
            match serde_json::from_str::<EventKey>(raw) {
                Ok(key) => layout.lines.push(Line {
                    event_id: key.event_id,
                    recorded_at: key.recorded_at,
                    raw: raw.to_string(),
                }),
                Err(error) => {
                    return Err(SillokError::datashape(
                        "invalid_event",
                        format!("{}: {error}", file.display()),
                    ));
                }
            }
        }
    }
    Ok(layout)
}

/// Renders one month file from `(recorded_at, id, raw)` lines.
pub fn render_month(mut lines: Vec<(Timestamp, EventId, &str)>) -> String {
    lines.sort_by_key(|(recorded, id, _)| (*recorded, *id));
    let mut out = String::with_capacity(lines.iter().map(|(_, _, raw)| raw.len() + 1).sum());
    for (_, _, raw) in lines {
        out.push_str(raw);
        out.push('\n');
    }
    out
}

/// Groups lines by month file name.
pub fn by_month<'a>(
    lines: impl IntoIterator<Item = (Timestamp, EventId, &'a str)>,
) -> BTreeMap<String, Vec<(Timestamp, EventId, &'a str)>> {
    let mut months: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for line in lines {
        months.entry(line.0.utc_month()).or_default().push(line);
    }
    months
}

fn read_capped(path: &Path, cap: u64) -> Result<String, SillokError> {
    let file = match std::fs::File::open(path) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut text = String::new();
    match file.take(cap + 1).read_to_string(&mut text) {
        Ok(read) if read as u64 > cap => Err(SillokError::datashape(
            "invalid_datashape",
            format!("{} exceeds the remote size limit", path.display()),
        )),
        Ok(_) => Ok(text),
        Err(error) => Err(error.into()),
    }
}
