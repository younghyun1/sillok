//! Property tests for replay and merge.

use std::collections::HashMap;

use proptest::prelude::*;
use sillok::domain::event::context::WorkContext;
use sillok::domain::event::envelope::{Event, EventBody};
use sillok::domain::event::kind::EventKind;
use sillok::domain::id::{EventId, RecordId};
use sillok::domain::merge;
use sillok::domain::record::RecordStatus;
use sillok::domain::reducer::replay::replay;
use sillok::domain::time::Timestamp;

/// One generated operation; indexes pick earlier records modulo their count.
#[derive(Debug, Clone)]
enum Op {
    Create { parent: Option<u8>, objective: bool },
    Amend { target: u8, status: u8 },
    Move { target: u8, parent: Option<u8> },
    Retract { target: u8 },
    Restore { target: u8 },
}

/// Generate each operation directly so shrinking preserves valid operation shapes.
fn operation() -> impl Strategy<Value = Op> {
    prop_oneof![
        (proptest::option::of(any::<u8>()), any::<bool>())
            .prop_map(|(parent, objective)| Op::Create { parent, objective }),
        (any::<u8>(), any::<u8>()).prop_map(|(target, status)| Op::Amend { target, status }),
        (any::<u8>(), proptest::option::of(any::<u8>()))
            .prop_map(|(target, parent)| Op::Move { target, parent }),
        any::<u8>().prop_map(|target| Op::Retract { target }),
        any::<u8>().prop_map(|target| Op::Restore { target }),
    ]
}

fn id(index: usize) -> RecordId {
    let mut bytes = [0u8; 16];
    bytes[8..].copy_from_slice(&(index as u64).to_be_bytes());
    RecordId::from_bytes(bytes)
}

fn build(ops: &[Op]) -> Vec<Event> {
    let mut created = 0usize;
    let mut events = Vec::new();
    for (step, op) in ops.iter().enumerate() {
        let pick = |raw: u8| match created {
            0 => None,
            count => Some(id(usize::from(raw) % count)),
        };
        let kind = match op {
            Op::Create { parent, objective } => {
                let record_id = id(created);
                let parent_id = match parent {
                    Some(raw) => pick(*raw),
                    None => None,
                };
                created += 1;
                match objective {
                    true => EventKind::ObjectiveAdded {
                        record_id,
                        parent_id,
                        text: format!("o{step}"),
                        tags: vec![],
                        status: RecordStatus::Active,
                    },
                    false => EventKind::TaskRecorded {
                        record_id,
                        parent_id,
                        text: format!("t{step}"),
                        purpose: None,
                        tags: vec![],
                        status: RecordStatus::Open,
                    },
                }
            }
            Op::Amend { target, status } => match pick(*target) {
                Some(record_id) => EventKind::RecordAmended {
                    record_id,
                    text: None,
                    status: Some(
                        [
                            RecordStatus::Open,
                            RecordStatus::Active,
                            RecordStatus::Blocked,
                            RecordStatus::Completed,
                        ][usize::from(*status) % 4],
                    ),
                    purpose: None,
                    clear_purpose: false,
                    tags: None,
                    note: Some(format!("n{step}")),
                },
                None => continue,
            },
            Op::Move { target, parent } => match pick(*target) {
                Some(record_id) => EventKind::RecordMoved {
                    record_id,
                    parent_id: match parent {
                        Some(raw) => pick(*raw),
                        None => None,
                    },
                },
                None => continue,
            },
            Op::Retract { target } => match pick(*target) {
                Some(record_id) => EventKind::RecordRetracted {
                    record_id,
                    reason: format!("r{step}"),
                },
                None => continue,
            },
            Op::Restore { target } => match pick(*target) {
                Some(record_id) => EventKind::RecordRestored { record_id },
                None => continue,
            },
        };
        let body = EventBody {
            event_id: EventId::from_bytes({
                let mut bytes = [0u8; 16];
                bytes[8..].copy_from_slice(&(step as u64).to_be_bytes());
                bytes
            }),
            event_at: Timestamp::from_millis(step as i64),
            recorded_at: Timestamp::from_millis(step as i64),
            actor: "prop".into(),
            context: WorkContext::default(),
            kind,
        };
        match Event::from_body(body) {
            Ok(event) => events.push(event),
            Err(_) => continue,
        }
    }
    events
}

fn check_replay_order(ops: &[Op], rotate: u8) {
    let events = build(ops);
    let forward = replay(events.iter());
    let mut shuffled = events.clone();
    shuffled.reverse();
    if !shuffled.is_empty() {
        let len = shuffled.len();
        shuffled.rotate_left(usize::from(rotate) % len);
    }
    let reordered = replay(shuffled.iter());
    assert_eq!(forward.records, reordered.records);
}

fn check_merge_split(ops: &[Op], split: u8) {
    let events = build(ops);
    let cut = match events.len() {
        0 => 0,
        len => usize::from(split) % (len + 1),
    };
    let (left, right) = events.split_at(cut);
    let left_raw: HashMap<EventId, &str> = left
        .iter()
        .map(|event| (event.id(), event.raw.as_str()))
        .collect();
    let right_raw: HashMap<EventId, &str> = right
        .iter()
        .map(|event| (event.id(), event.raw.as_str()))
        .collect();
    let into_left = merge::plan(&left_raw, right.to_vec());
    let into_right = merge::plan(&right_raw, left.to_vec());
    assert_eq!(into_left.additions.len() + left.len(), events.len());
    assert_eq!(into_right.additions.len() + right.len(), events.len());
    assert!(into_left.conflicts.is_empty() && into_right.conflicts.is_empty());
}

proptest! {
    #[test]
    fn replay_ignores_arrival_order(
        ops in proptest::collection::vec(operation(), 0..128),
        rotate in any::<u8>(),
    ) {
        check_replay_order(&ops, rotate);
    }

    #[test]
    fn merge_of_any_split_restores_the_whole(
        ops in proptest::collection::vec(operation(), 0..128),
        split in any::<u8>(),
    ) {
        check_merge_split(&ops, split);
    }
}
