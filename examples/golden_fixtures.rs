//! Writes the 0.10 golden fixtures used by the 1.0 import tests.
//!
//! Run once against the 0.10 codebase: `cargo run --example golden_fixtures -- <out_dir>`.
//! The archive covers every v2 event kind with fixed ids and timestamps, including a
//! day opened twice under different timezone labels and a day retracted through
//! `amend`, so later versions can prove they import every shape 0.10 could persist.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use sillok::archive_codec::{LEGACY_ZSTD_LEVEL, encode_archive, encode_sync_archive};
use sillok::domain::archive::{ARCHIVE_SCHEMA_VERSION, Archive};
use sillok::domain::event::{ChronicleEvent, EventKind, RecordStatus, WorkContext};
use sillok::domain::id::ChronicleId;
use sillok::domain::time::{DayKey, Timestamp};
use sillok::domain::view::ChronicleView;
use sillok::error::SillokError;
use sillok::storage::sql::store::SqlStore;

const BASE_MS: i64 = 1_778_666_400_000; // 2026-05-13T10:00:00Z

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), SillokError> {
    let out = match std::env::args().nth(1) {
        Some(value) => PathBuf::from(value),
        None => {
            return Err(SillokError::new(
                "usage",
                "usage: golden_fixtures <out_dir>",
            ));
        }
    };
    if let Err(error) = fs::create_dir_all(&out) {
        return Err(error.into());
    }
    let archive = fixture_archive();
    let visible_records = match ChronicleView::build(&archive) {
        Ok(view) => view.visible_records(),
        Err(error) => return Err(error),
    };
    let legacy = match encode_archive(&archive, LEGACY_ZSTD_LEVEL) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let sync = match encode_sync_archive(&archive) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = fs::write(out.join("archive.slk.zst"), legacy) {
        return Err(error.into());
    }
    if let Err(error) = fs::write(out.join("sync.slk.zst"), sync) {
        return Err(error.into());
    }
    let store = out.join("store.db");
    if let Err(error) = remove_if_exists(&store) {
        return Err(error);
    }
    if let Err(error) = SqlStore::new(store.clone()).import_archive(&archive).await {
        return Err(error);
    }

    let expected = json!({
        "archive_id": archive.archive_id,
        "created_at": archive.created_at,
        "events": archive.events,
        "visible_records": visible_records,
    });
    let encoded = match serde_json::to_vec_pretty(&expected) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = fs::write(out.join("expected.json"), encoded) {
        return Err(error.into());
    }
    println!("wrote fixtures to {}", out.display());
    Ok(())
}

fn fixture_archive() -> Archive {
    let archive_id = id(0, 0x01);
    let day_local = id(1, 0x02);
    let day_denver = id(2, 0x03);
    let day_next = id(3, 0x04);
    let objective = id(4, 0x05);
    let task_under_objective = id(5, 0x06);
    let task_under_denver = id(6, 0x07);
    let task_next_day = id(7, 0x08);
    let task_retracted = id(8, 0x09);
    let task_unlinked = id(9, 0x0a);

    let kinds = vec![
        EventKind::ArchiveInitialized { archive_id },
        EventKind::DayOpened {
            day_id: day_local,
            day_key: day_key("2026-05-13", "local"),
        },
        EventKind::ObjectiveAdded {
            objective_id: objective,
            day_id: day_local,
            text: "Ship the storage refactor".to_string(),
            tags: vec!["rust".to_string(), "storage".to_string()],
        },
        EventKind::TaskRecorded {
            task_id: task_under_objective,
            day_id: day_local,
            parent_id: objective,
            text: "Split reducer from indexing".to_string(),
            purpose: Some("Keep replay cheap".to_string()),
            tags: vec!["rust".to_string()],
            status: RecordStatus::Completed,
        },
        EventKind::DayOpened {
            day_id: day_denver,
            day_key: day_key("2026-05-13", "America/Denver"),
        },
        EventKind::TaskRecorded {
            task_id: task_under_denver,
            day_id: day_denver,
            parent_id: day_denver,
            text: "Backfilled with an explicit timezone".to_string(),
            purpose: None,
            tags: Vec::new(),
            status: RecordStatus::Open,
        },
        EventKind::TaskAmended {
            record_id: task_under_denver,
            text: Some("Backfilled with an explicit tz".to_string()),
            status: Some(RecordStatus::Active),
            purpose: Some("Exercise amend".to_string()),
            tags: Some(vec!["docs".to_string(), "time".to_string()]),
        },
        EventKind::TaskRecorded {
            task_id: task_retracted,
            day_id: day_local,
            parent_id: day_local,
            text: "Recorded against the wrong objective".to_string(),
            purpose: None,
            tags: Vec::new(),
            status: RecordStatus::Completed,
        },
        EventKind::TaskRetracted {
            record_id: task_retracted,
            reason: "Wrong objective".to_string(),
        },
        EventKind::TaskLinked {
            child_id: task_under_denver,
            parent_id: objective,
        },
        EventKind::ObjectiveCompleted {
            objective_id: objective,
            note: Some("All scoped work is complete".to_string()),
        },
        EventKind::DayOpened {
            day_id: day_next,
            day_key: day_key("2026-05-14", "UTC"),
        },
        EventKind::TaskRecorded {
            task_id: task_next_day,
            day_id: day_next,
            parent_id: task_under_objective,
            text: "Follow-up on the next day".to_string(),
            purpose: None,
            tags: vec!["followup".to_string()],
            status: RecordStatus::Blocked,
        },
        EventKind::TaskRecorded {
            task_id: task_unlinked,
            day_id: day_next,
            parent_id: day_next,
            text: "Detached from its day".to_string(),
            purpose: None,
            tags: Vec::new(),
            status: RecordStatus::Open,
        },
        EventKind::TaskUnlinked {
            child_id: task_unlinked,
        },
        EventKind::TaskAmended {
            record_id: day_next,
            text: None,
            status: Some(RecordStatus::Retracted),
            purpose: None,
            tags: None,
        },
    ];

    let events = kinds
        .into_iter()
        .enumerate()
        .map(|(index, kind)| {
            let offset = match i64::try_from(index) {
                Ok(value) => value * 60_000,
                Err(_) => 0,
            };
            let at = Timestamp::from_millis(BASE_MS + offset);
            ChronicleEvent {
                event_id: id(100 + index as u16, 0x40),
                event_at: at,
                recorded_at: Timestamp::from_millis(BASE_MS + offset + 5),
                actor: "fixture".to_string(),
                context: context(index),
                kind,
            }
        })
        .collect();

    Archive {
        schema_version: ARCHIVE_SCHEMA_VERSION,
        archive_id,
        created_at: Timestamp::from_millis(BASE_MS),
        events,
    }
}

fn id(sequence: u16, fill: u8) -> ChronicleId {
    let millis = match u64::try_from(BASE_MS) {
        Ok(value) => value + u64::from(sequence),
        Err(_) => u64::from(sequence),
    };
    let mut random = [fill; 10];
    random[0] = (sequence >> 8) as u8;
    random[1] = (sequence & 0xff) as u8;
    let uuid = uuid::Builder::from_unix_timestamp_millis(millis, &random).into_uuid();
    ChronicleId::from_bytes(*uuid.as_bytes())
}

fn day_key(date: &str, timezone: &str) -> DayKey {
    DayKey {
        date: date.to_string(),
        timezone: timezone.to_string(),
    }
}

fn context(index: usize) -> WorkContext {
    if index % 3 == 0 {
        WorkContext {
            cwd: Some("/home/fixture/scratch".to_string()),
            git_root: None,
            git_branch: None,
            git_head: None,
            git_remote: None,
        }
    } else {
        WorkContext {
            cwd: Some("/home/fixture/repo/src".to_string()),
            git_root: Some("/home/fixture/repo".to_string()),
            git_branch: Some("main".to_string()),
            git_head: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            git_remote: Some("https://user:secret@example.com/fixture/repo.git".to_string()),
        }
    }
}

fn remove_if_exists(path: &Path) -> Result<(), SillokError> {
    for suffix in ["", "-wal", "-shm"] {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(suffix);
        match fs::remove_file(PathBuf::from(candidate)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
