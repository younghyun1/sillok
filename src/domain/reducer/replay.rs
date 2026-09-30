//! Order-independent replay of an event set into current records.
//!
//! The result depends only on which events exist, never on the order they
//! arrive in, so replicas that merged the same events agree exactly:
//! 1. creation events build records (first by `(recorded_at, event_id)` wins);
//! 2. creation parents are validated, and missing parents or cycles become top-level;
//! 3. mutations apply in `(recorded_at, event_id)` order, last writer wins;
//!    a move that would create a cycle or points at a missing record is skipped.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::domain::event::envelope::Event;
use crate::domain::event::kind::EventKind;
use crate::domain::id::RecordId;
use crate::domain::record::Record;
use crate::domain::reducer::rules;

/// Records derived from an event set, plus what replay had to repair.
#[derive(Debug, Default)]
pub struct Projection {
    pub records: HashMap<RecordId, Record>,
    pub warnings: Vec<String>,
    pub unknown_events: usize,
}

/// Replays events into records.
pub fn replay<'a>(events: impl IntoIterator<Item = &'a Event>) -> Projection {
    let mut creations = Vec::new();
    let mut mutations = Vec::new();
    let mut projection = Projection::default();
    for event in events {
        match &event.body.kind {
            EventKind::Unknown => projection.unknown_events += 1,
            kind if kind.created_kind().is_some() => creations.push(event),
            _ => mutations.push(event),
        }
    }
    creations.sort_by_key(|event| event.order_key());
    mutations.sort_by_key(|event| event.order_key());

    for event in creations {
        match rules::create(&event.body) {
            Some(record) => match projection.records.entry(record.id) {
                Entry::Occupied(_) => projection.warnings.push(format!(
                    "record `{}` has more than one creation event; kept the earliest",
                    record.id
                )),
                Entry::Vacant(slot) => {
                    slot.insert(record);
                }
            },
            None => {}
        }
    }
    repair_creation_parents(&mut projection);

    for event in mutations {
        let record_id = match event.body.kind.record_id() {
            Some(value) => value,
            None => continue,
        };
        if !projection.records.contains_key(&record_id) {
            projection.warnings.push(format!(
                "event `{}` targets missing record `{record_id}`",
                event.id()
            ));
            continue;
        }
        match &event.body.kind {
            EventKind::RecordMoved { parent_id, .. } => {
                apply_move(&mut projection, event, record_id, *parent_id);
            }
            _ => {
                if let Some(record) = projection.records.get_mut(&record_id) {
                    rules::apply(record, &event.body);
                }
            }
        }
    }
    projection
}

/// Returns whether `candidate` is `record` or one of its descendants, by
/// walking the candidate's ancestors. Bounded by the record count, so a
/// corrupt graph cannot loop forever.
pub fn is_self_or_descendant(
    records: &HashMap<RecordId, Record>,
    record: RecordId,
    candidate: RecordId,
) -> bool {
    let mut current = Some(candidate);
    let mut steps = 0usize;
    while let Some(id) = current {
        if id == record {
            return true;
        }
        steps += 1;
        if steps > records.len() {
            return true;
        }
        current = match records.get(&id) {
            Some(value) => value.parent,
            None => None,
        };
    }
    false
}

fn apply_move(
    projection: &mut Projection,
    event: &Event,
    record_id: RecordId,
    parent_id: Option<RecordId>,
) {
    if let Some(parent) = parent_id {
        if !projection.records.contains_key(&parent) {
            projection.warnings.push(format!(
                "move `{}` points at missing parent `{parent}`; skipped",
                event.id()
            ));
            return;
        }
        if is_self_or_descendant(&projection.records, record_id, parent) {
            projection.warnings.push(format!(
                "move `{}` would create a cycle; skipped",
                event.id()
            ));
            return;
        }
    }
    if let Some(record) = projection.records.get_mut(&record_id) {
        record.parent = parent_id;
        rules::touch(record, &event.body);
    }
}

/// Makes records with a missing parent top-level and breaks cycles. Records
/// are visited in id order so every replica repairs the same way.
fn repair_creation_parents(projection: &mut Projection) {
    let mut ids: Vec<RecordId> = projection.records.keys().copied().collect();
    ids.sort();
    for id in &ids {
        let parent = match projection.records.get(id) {
            Some(record) => record.parent,
            None => None,
        };
        if let Some(parent_id) = parent
            && !projection.records.contains_key(&parent_id)
        {
            projection.warnings.push(format!(
                "record `{id}` points at missing parent `{parent_id}`; shown top-level"
            ));
            if let Some(record) = projection.records.get_mut(id) {
                record.parent = None;
            }
        }
    }
    for id in &ids {
        // A walk may reach a cycle it is not part of; only the cycle's own
        // members are candidates, and the smallest id is detached so every
        // replica breaks it at the same place.
        loop {
            let mut path: Vec<RecordId> = Vec::new();
            let mut position: HashMap<RecordId, usize> = HashMap::new();
            let mut current = Some(*id);
            let mut cycle_start = None;
            while let Some(step) = current {
                if let Some(index) = position.get(&step) {
                    cycle_start = Some(*index);
                    break;
                }
                position.insert(step, path.len());
                path.push(step);
                current = match projection.records.get(&step) {
                    Some(record) => record.parent,
                    None => None,
                };
            }
            let members = match cycle_start {
                Some(index) => &path[index..],
                None => break,
            };
            let breaker = match members.iter().min() {
                Some(value) => *value,
                None => break,
            };
            projection.warnings.push(format!(
                "record `{breaker}` closed a parent cycle; shown top-level"
            ));
            if let Some(record) = projection.records.get_mut(&breaker) {
                record.parent = None;
            }
        }
    }
}
