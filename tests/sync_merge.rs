use sillok::domain::archive::Archive;
use sillok::domain::event::{ChronicleEvent, EventKind, RecordStatus, WorkContext};
use sillok::domain::id::ChronicleId;
use sillok::domain::time::{DayKey, Timestamp};
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
fn rejects_archive_id_mismatch() -> Result<(), Box<dyn std::error::Error>> {
    let local = base_archive();
    let remote = base_archive();
    let error = match merge_archives(Some(&local), Some(&remote)) {
        Ok(_) => return Err(Box::new(std::io::Error::other("merge succeeded"))),
        Err(error) => error,
    };
    assert_eq!(error.code(), "sync_archive_mismatch");
    Ok(())
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
