use sillok::domain::archive::Archive;
use sillok::domain::event::{ChronicleEvent, EventKind, RecordStatus, WorkContext};
use sillok::domain::id::ChronicleId;
use sillok::domain::time::{DayKey, Timestamp};
use sillok::domain::view::ChronicleView;

fn context() -> WorkContext {
    WorkContext {
        cwd: Some("/tmp/sillok-test".to_string()),
        git_root: None,
        git_branch: None,
        git_head: None,
        git_remote: None,
    }
}

fn archive() -> Archive {
    Archive::new(Timestamp::from_millis(1), "test".to_string(), context())
}

#[test]
fn derives_day_task_tree() -> Result<(), Box<dyn std::error::Error>> {
    let mut archive = archive();
    let day_id = ChronicleId::new_v7();
    let task_id = ChronicleId::new_v7();
    archive.push(ChronicleEvent::new(
        Timestamp::from_millis(2),
        Timestamp::from_millis(2),
        "test".to_string(),
        context(),
        EventKind::DayOpened {
            day_id,
            day_key: DayKey {
                date: "2026-05-13".to_string(),
                timezone: "local".to_string(),
            },
        },
    ));
    archive.push(ChronicleEvent::new(
        Timestamp::from_millis(3),
        Timestamp::from_millis(3),
        "test".to_string(),
        context(),
        EventKind::TaskRecorded {
            task_id,
            day_id,
            parent_id: day_id,
            text: "implemented storage".to_string(),
            purpose: None,
            tags: vec!["rust".to_string()],
            status: RecordStatus::Completed,
        },
    ));

    let view = ChronicleView::build(&archive)?;
    let tree = view.tree(day_id)?;
    assert_eq!(tree.children.len(), 1);
    assert_eq!(tree.children[0].record.record_id, task_id);
    Ok(())
}

#[test]
fn query_uses_created_time_and_hides_retracted_records() -> Result<(), Box<dyn std::error::Error>> {
    let mut archive = archive();
    let day_id = ChronicleId::new_v7();
    let task_id = ChronicleId::new_v7();
    archive.push(ChronicleEvent::new(
        Timestamp::from_millis(2),
        Timestamp::from_millis(2),
        "test".to_string(),
        context(),
        EventKind::DayOpened {
            day_id,
            day_key: DayKey {
                date: "2026-05-13".to_string(),
                timezone: "local".to_string(),
            },
        },
    ));
    archive.push(ChronicleEvent::new(
        Timestamp::from_millis(3),
        Timestamp::from_millis(3),
        "test".to_string(),
        context(),
        EventKind::TaskRecorded {
            task_id,
            day_id,
            parent_id: day_id,
            text: "temporary".to_string(),
            purpose: None,
            tags: Vec::new(),
            status: RecordStatus::Completed,
        },
    ));
    archive.push(ChronicleEvent::new(
        Timestamp::from_millis(4),
        Timestamp::from_millis(4),
        "test".to_string(),
        context(),
        EventKind::TaskRetracted {
            record_id: task_id,
            reason: "mistake".to_string(),
        },
    ));

    let view = ChronicleView::build(&archive)?;
    let records = view.query(
        Timestamp::from_millis(0),
        Timestamp::from_millis(10),
        None,
        None,
        None,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, day_id);
    Ok(())
}

#[test]
fn relinking_replaces_the_previous_parent_bucket() -> Result<(), Box<dyn std::error::Error>> {
    let mut archive = archive();
    let day_id = ChronicleId::new_v7();
    let first_parent_id = ChronicleId::new_v7();
    let second_parent_id = ChronicleId::new_v7();
    let child_id = ChronicleId::new_v7();
    archive.push(ChronicleEvent::new(
        Timestamp::from_millis(2),
        Timestamp::from_millis(2),
        "test".to_string(),
        context(),
        EventKind::DayOpened {
            day_id,
            day_key: DayKey {
                date: "2026-05-13".to_string(),
                timezone: "local".to_string(),
            },
        },
    ));
    for (task_id, text) in [
        (first_parent_id, "first parent"),
        (second_parent_id, "second parent"),
        (child_id, "child"),
    ] {
        archive.push(ChronicleEvent::new(
            Timestamp::from_millis(3),
            Timestamp::from_millis(3),
            "test".to_string(),
            context(),
            EventKind::TaskRecorded {
                task_id,
                day_id,
                parent_id: day_id,
                text: text.to_string(),
                purpose: None,
                tags: Vec::new(),
                status: RecordStatus::Open,
            },
        ));
    }
    archive.push(ChronicleEvent::new(
        Timestamp::from_millis(4),
        Timestamp::from_millis(4),
        "test".to_string(),
        context(),
        EventKind::TaskLinked {
            child_id,
            parent_id: first_parent_id,
        },
    ));
    archive.push(ChronicleEvent::new(
        Timestamp::from_millis(5),
        Timestamp::from_millis(5),
        "test".to_string(),
        context(),
        EventKind::TaskLinked {
            child_id,
            parent_id: second_parent_id,
        },
    ));

    let view = ChronicleView::build(&archive)?;
    let first_tree = view.tree(first_parent_id)?;
    let second_tree = view.tree(second_parent_id)?;

    assert_eq!(first_tree.children.len(), 0);
    assert_eq!(second_tree.children.len(), 1);
    assert_eq!(second_tree.children[0].record.record_id, child_id);
    Ok(())
}
