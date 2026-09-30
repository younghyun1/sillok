//! Replay behavior, including order independence and graph repair.

use crate::domain::event::context::WorkContext;
use crate::domain::event::envelope::{Event, EventBody};
use crate::domain::event::kind::EventKind;
use crate::domain::id::{EventId, RecordId};
use crate::domain::record::RecordStatus;
use crate::domain::reducer::replay::replay;
use crate::domain::time::Timestamp;
use crate::error::SillokError;

fn event(at: i64, kind: EventKind) -> Result<Event, SillokError> {
    Event::from_body(EventBody {
        event_id: EventId::new_v7(),
        event_at: Timestamp::from_millis(at),
        recorded_at: Timestamp::from_millis(at),
        actor: "test".into(),
        context: WorkContext::default(),
        kind,
    })
}

fn task(at: i64, id: RecordId, parent: Option<RecordId>) -> Result<Event, SillokError> {
    event(
        at,
        EventKind::TaskRecorded {
            record_id: id,
            parent_id: parent,
            text: format!("task {at}"),
            purpose: None,
            tags: vec![],
            status: RecordStatus::Active,
        },
    )
}

fn collect(items: Vec<Result<Event, SillokError>>) -> Result<Vec<Event>, SillokError> {
    let mut events = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Ok(value) => events.push(value),
            Err(error) => return Err(error),
        }
    }
    Ok(events)
}

#[test]
fn replay_is_order_independent() -> Result<(), SillokError> {
    let (a, b) = (RecordId::new_v7(), RecordId::new_v7());
    let events = match collect(vec![
        task(1, a, None),
        task(2, b, Some(a)),
        event(
            3,
            EventKind::RecordAmended {
                record_id: b,
                text: None,
                status: Some(RecordStatus::Completed),
                purpose: None,
                clear_purpose: false,
                tags: None,
                note: Some("done".into()),
            },
        ),
        event(
            4,
            EventKind::RecordMoved {
                record_id: b,
                parent_id: None,
            },
        ),
    ]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let forward = replay(events.iter());
    let backward = replay(events.iter().rev());
    assert_eq!(forward.records, backward.records);
    let record = forward.records.get(&b);
    assert!(matches!(record, Some(r) if r.status == RecordStatus::Completed && r.parent.is_none()));
    Ok(())
}

#[test]
fn cyclic_move_is_skipped() -> Result<(), SillokError> {
    let (a, b) = (RecordId::new_v7(), RecordId::new_v7());
    let events = match collect(vec![
        task(1, a, None),
        task(2, b, Some(a)),
        event(
            3,
            EventKind::RecordMoved {
                record_id: a,
                parent_id: Some(b),
            },
        ),
    ]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let projection = replay(events.iter());
    assert!(matches!(projection.records.get(&a), Some(r) if r.parent.is_none()));
    assert_eq!(projection.warnings.len(), 1);
    Ok(())
}

#[test]
fn missing_parent_becomes_top_level() -> Result<(), SillokError> {
    let a = RecordId::new_v7();
    let events = match collect(vec![task(1, a, Some(RecordId::new_v7()))]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let projection = replay(events.iter());
    assert!(matches!(projection.records.get(&a), Some(r) if r.parent.is_none()));
    Ok(())
}

#[test]
fn retract_and_restore_round_trip() -> Result<(), SillokError> {
    let a = RecordId::new_v7();
    let events = match collect(vec![
        task(1, a, None),
        event(
            2,
            EventKind::RecordRetracted {
                record_id: a,
                reason: "oops".into(),
            },
        ),
        event(3, EventKind::RecordRestored { record_id: a }),
    ]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let projection = replay(events.iter());
    let record = projection.records.get(&a);
    assert!(
        matches!(record, Some(r) if r.status == RecordStatus::Active && r.retraction_reason.is_none())
    );
    Ok(())
}

#[test]
fn unknown_events_are_counted_not_applied() -> Result<(), SillokError> {
    let raw = r#"{"event_id":"01a0f01e-f6f0-7091-a38c-09f1b8f3c285","event_at":"2026-09-30T00:00:00.000Z","recorded_at":"2026-09-30T00:00:00.000Z","actor":"future","kind":{"type":"record_starred"}}"#;
    let unknown = match Event::parse(raw.to_string()) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let projection = replay([&unknown]);
    assert_eq!(projection.unknown_events, 1);
    assert!(projection.records.is_empty());
    Ok(())
}

#[test]
fn creation_cycles_detach_only_a_cycle_member() -> Result<(), SillokError> {
    // Fixed ids make A the smallest, so a naive walk from A would detach it.
    let fixed = |n: u8| {
        let mut bytes = [0u8; 16];
        bytes[15] = n;
        RecordId::from_bytes(bytes)
    };
    let (a, b, c) = (fixed(1), fixed(2), fixed(3));
    let events = match collect(vec![
        task(1, a, Some(b)),
        task(2, b, Some(c)),
        task(3, c, Some(b)),
    ]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let projection = replay(events.iter());
    assert!(matches!(projection.records.get(&a), Some(r) if r.parent == Some(b)));
    assert!(matches!(projection.records.get(&b), Some(r) if r.parent.is_none()));
    assert!(matches!(projection.records.get(&c), Some(r) if r.parent == Some(b)));
    Ok(())
}
