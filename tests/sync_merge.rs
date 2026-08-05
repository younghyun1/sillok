use sillok::domain::archive::Archive;
use sillok::domain::event::{ChronicleEvent, EventKind, RecordKind, RecordStatus, WorkContext};
use sillok::domain::id::ChronicleId;
use sillok::domain::time::{DayKey, Timestamp};
use sillok::domain::view::ChronicleView;
use sillok::sync::merge::merge_archives;

fn context() -> WorkContext {
    WorkContext {
        cwd: Some("/tmp/sillok-merge-test".to_string()),
        git_root: None,
        git_branch: None,
        git_head: None,
        git_remote: None,
    }
}

fn base_archive() -> Archive {
    Archive::new(Timestamp::from_millis(1), "test".to_string(), context())
}

#[test]
fn deduplicates_identical_events() -> Result<(), Box<dyn std::error::Error>> {
    let archive = base_archive();
    let merged = merge_archives(Some(&archive), Some(&archive))?;
    let Some(result) = merged.archive else {
        return Err(Box::new(std::io::Error::other("missing archive")));
    };
    assert_eq!(result.events.len(), archive.events.len());
    assert!(!merged.merged);
    Ok(())
}

#[test]
fn merges_divergent_same_archive_events() -> Result<(), Box<dyn std::error::Error>> {
    let mut local = base_archive();
    let mut remote = local.clone();
    let day_id = ChronicleId::new_v7();
    local.events.push(day_event(day_id));
    remote.events.push(day_event(ChronicleId::new_v7()));

    let merged = merge_archives(Some(&local), Some(&remote))?;
    let Some(result) = merged.archive else {
        return Err(Box::new(std::io::Error::other("missing archive")));
    };
    assert_eq!(result.events.len(), 3);
    assert!(merged.merged);
    Ok(())
}

#[test]
fn orders_dependencies_before_dependents() -> Result<(), Box<dyn std::error::Error>> {
    let mut local = base_archive();
    let mut remote = local.clone();
    let day_id = ChronicleId::new_v7();
    let task_id = ChronicleId::new_v7();
    local.events.push(task_event(task_id, day_id));
    remote.events.push(day_event(day_id));

    let merged = merge_archives(Some(&local), Some(&remote))?;
    let Some(result) = merged.archive else {
        return Err(Box::new(std::io::Error::other("missing archive")));
    };
    assert!(matches!(result.events[1].kind, EventKind::DayOpened { .. }));
    assert!(matches!(
        result.events[2].kind,
        EventKind::TaskRecorded { .. }
    ));
    Ok(())
}

#[test]
fn meshes_independent_archives_under_one_day() -> Result<(), Box<dyn std::error::Error>> {
    let (older, older_day) = independent_archive(1_000, "older note");
    let (newer, newer_day) = independent_archive(2_000, "newer note");

    let outcome = merge_archives(Some(&older), Some(&newer))?;
    let Some(merged) = outcome.archive else {
        return Err(Box::new(std::io::Error::other("missing archive")));
    };
    assert!(outcome.merged);
    // The older identity survives so replicas converge on one archive_id.
    assert_eq!(merged.archive_id, older.archive_id);
    assert_eq!(merged.created_at, older.created_at);

    let view = ChronicleView::build(&merged)?;
    let days: Vec<_> = view
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Day)
        .collect();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].record_id, older_day);
    assert_eq!(view.canonical_id(newer_day), older_day);

    let mut texts: Vec<_> = view
        .records_for_day(older_day)
        .into_iter()
        .map(|record| record.text)
        .collect();
    texts.sort();
    assert_eq!(texts, vec!["newer note", "older note"]);
    Ok(())
}

#[test]
fn mesh_is_symmetric() -> Result<(), Box<dyn std::error::Error>> {
    let (left, _) = independent_archive(1_000, "left note");
    let (right, _) = independent_archive(2_000, "right note");

    let forward = merge_archives(Some(&left), Some(&right))?.archive;
    let backward = merge_archives(Some(&right), Some(&left))?.archive;
    assert_eq!(forward, backward);
    Ok(())
}

#[test]
fn mesh_rejects_conflicting_payloads_for_one_event_id() -> Result<(), Box<dyn std::error::Error>> {
    let (base, _) = independent_archive(1_000, "original note");
    let mut tampered = base.clone();
    if let Some(event) = tampered.events.last_mut()
        && let EventKind::TaskRecorded { text, .. } = &mut event.kind
    {
        *text = "tampered note".to_string();
    }

    let error = match merge_archives(Some(&base), Some(&tampered)) {
        Ok(_) => return Err(Box::new(std::io::Error::other("merge succeeded"))),
        Err(error) => error,
    };
    assert_eq!(error.code(), "sync_merge_conflict");
    Ok(())
}

/// Builds an independently-initialized archive holding one day and one task.
fn independent_archive(created_ms: i64, text: &str) -> (Archive, ChronicleId) {
    let created = Timestamp::from_millis(created_ms);
    let mut archive = Archive::new(created, "test".to_string(), context());
    let day_id = ChronicleId::new_v7();
    archive.push(ChronicleEvent::new(
        created,
        created,
        "test".to_string(),
        context(),
        EventKind::DayOpened {
            day_id,
            day_key: DayKey {
                date: "2026-05-13".to_string(),
                timezone: "UTC".to_string(),
            },
        },
    ));
    let noted = Timestamp::from_millis(created_ms + 100);
    archive.push(ChronicleEvent::new(
        noted,
        noted,
        "test".to_string(),
        context(),
        EventKind::TaskRecorded {
            task_id: ChronicleId::new_v7(),
            day_id,
            parent_id: day_id,
            text: text.to_string(),
            purpose: None,
            tags: Vec::new(),
            status: RecordStatus::Completed,
        },
    ));
    (archive, day_id)
}

fn day_event(day_id: ChronicleId) -> ChronicleEvent {
    ChronicleEvent::new(
        Timestamp::from_millis(2),
        Timestamp::from_millis(2),
        "test".to_string(),
        context(),
        EventKind::DayOpened {
            day_id,
            day_key: DayKey {
                date: "2026-05-13".to_string(),
                timezone: "UTC".to_string(),
            },
        },
    )
}

fn task_event(task_id: ChronicleId, day_id: ChronicleId) -> ChronicleEvent {
    ChronicleEvent::new(
        Timestamp::from_millis(3),
        Timestamp::from_millis(3),
        "test".to_string(),
        context(),
        EventKind::TaskRecorded {
            task_id,
            day_id,
            parent_id: day_id,
            text: "sync task".to_string(),
            purpose: None,
            tags: Vec::new(),
            status: RecordStatus::Completed,
        },
    )
}
