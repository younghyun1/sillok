//! Single-event writes: validate, append, and update the projection in one
//! `BEGIN IMMEDIATE` transaction so concurrent agents cannot interleave a
//! check and its write.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use crate::domain::event::context::WorkContext;
use crate::domain::event::envelope::Event;
use crate::domain::event::kind::EventKind;
use crate::domain::id::RecordId;
use crate::domain::record::{Record, RecordKind, RecordStatus};
use crate::domain::reducer::rules;
use crate::domain::time::Timestamp;
use crate::error::SillokError;
use crate::storage::sqlite::open::Store;
use crate::storage::sqlite::{events, records};

/// Upper bound on ancestor walks; far above any real tree depth.
const MAX_DEPTH: usize = 10_000;

/// Who, when, and where for a new event.
#[derive(Debug, Clone)]
pub struct Stamp {
    pub event_at: Timestamp,
    pub recorded_at: Timestamp,
    /// Whether `event_at` came from `--at`; otherwise it follows `recorded_at`.
    pub backfilled: bool,
    pub actor: String,
    pub context: WorkContext,
}

/// Now, or the earliest instant after `floor`, whichever is later; never
/// earlier than the caller's own stamp.
pub fn next_recorded_at(stamped: Timestamp, floor: Option<Timestamp>) -> Timestamp {
    let now = Timestamp::now().max(stamped);
    match floor {
        Some(value) if value >= now => Timestamp::from_millis(value.as_millis() + 1),
        Some(_) | None => now,
    }
}

fn updated_at(conn: &Connection, id: RecordId) -> Result<Option<Timestamp>, SillokError> {
    match conn
        .query_row(
            "SELECT record_updated_at_ms FROM record WHERE record_id = ?1",
            [id.as_bytes().to_vec()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
    {
        Ok(value) => Ok(value.map(Timestamp::from_millis)),
        Err(error) => Err(error.into()),
    }
}

impl Store {
    /// Appends one event and returns the record it produced or changed.
    pub fn append(&mut self, stamp: Stamp, kind: EventKind) -> Result<Record, SillokError> {
        let tx = match self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
        {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        // Replay orders writes by recorded_at, so it is taken only once this
        // process holds the write lock and is kept above the target record's
        // updated_at. Otherwise a writer that waited on the lock, or a synced
        // event from a machine whose clock runs ahead, would make the live
        // projection disagree with a replay of the same events.
        let floor = match kind.created_kind() {
            Some(_) => None,
            None => match kind.record_id() {
                Some(id) => match updated_at(&tx, id) {
                    Ok(value) => value,
                    Err(error) => return Err(error),
                },
                None => None,
            },
        };
        let recorded_at = next_recorded_at(stamp.recorded_at, floor);
        let event_at = match stamp.backfilled {
            true => stamp.event_at,
            false => recorded_at,
        };
        let event = match Event::create(event_at, recorded_at, stamp.actor, stamp.context, kind) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let record = match next_state(&tx, &event) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        if let Err(error) = events::insert(&tx, &event) {
            return Err(error);
        }
        if let Err(error) = records::upsert(&tx, &record) {
            return Err(error);
        }
        match tx.commit() {
            Ok(()) => Ok(record),
            Err(error) => Err(error.into()),
        }
    }
}

/// Validates the event against current state and computes the new record.
fn next_state(conn: &Connection, event: &Event) -> Result<Record, SillokError> {
    let body = &event.body;
    if let Some(kind) = body.kind.created_kind() {
        let parent = match &body.kind {
            EventKind::ObjectiveAdded { parent_id, .. }
            | EventKind::TaskRecorded { parent_id, .. } => *parent_id,
            _ => None,
        };
        if let Some(parent_id) = parent
            && let Err(error) = check_parent(conn, kind, parent_id)
        {
            return Err(error);
        }
        return match rules::create(body) {
            Some(record) => Ok(record),
            None => Err(SillokError::operation(
                "invalid_event",
                "creation event has no record",
            )),
        };
    }
    let record_id = match body.kind.record_id() {
        Some(value) => value,
        None => {
            return Err(SillokError::operation(
                "invalid_event",
                "event has no record",
            ));
        }
    };
    let mut record = match records::fetch(conn, record_id) {
        Ok(Some(value)) => value,
        Ok(None) => return Err(SillokError::RecordNotFound(record_id.to_string())),
        Err(error) => return Err(error),
    };
    match &body.kind {
        EventKind::RecordRestored { .. } => {
            if record.status != RecordStatus::Retracted {
                return Err(SillokError::operation(
                    "not_retracted",
                    format!("record `{record_id}` is not retracted"),
                ));
            }
        }
        _ => {
            if record.status == RecordStatus::Retracted {
                return Err(SillokError::RecordRetracted(record_id.to_string()));
            }
        }
    }
    match &body.kind {
        EventKind::RecordMoved { parent_id, .. } => {
            if let Some(parent) = parent_id {
                if let Err(error) = check_parent(conn, record.kind, *parent) {
                    return Err(error);
                }
                match is_self_or_descendant(conn, record_id, *parent) {
                    Ok(true) => {
                        return Err(SillokError::operation(
                            "parent_cycle",
                            format!("moving `{record_id}` under `{parent}` would create a cycle"),
                        ));
                    }
                    Ok(false) => {}
                    Err(error) => return Err(error),
                }
            }
            record.parent = *parent_id;
            rules::touch(&mut record, body);
        }
        _ => {
            rules::apply(&mut record, body);
        }
    }
    Ok(record)
}

/// Parents must exist and be visible; objectives nest only under objectives.
fn check_parent(
    conn: &Connection,
    child: RecordKind,
    parent_id: RecordId,
) -> Result<(), SillokError> {
    let parent = match records::fetch(conn, parent_id) {
        Ok(Some(value)) => value,
        Ok(None) => return Err(SillokError::RecordNotFound(parent_id.to_string())),
        Err(error) => return Err(error),
    };
    if parent.status == RecordStatus::Retracted {
        return Err(SillokError::RecordRetracted(parent_id.to_string()));
    }
    if child == RecordKind::Objective && parent.kind != RecordKind::Objective {
        return Err(SillokError::operation(
            "invalid_parent",
            format!("objective parent `{parent_id}` must be an objective"),
        ));
    }
    Ok(())
}

/// Walks up from `candidate`; true when it reaches `record`.
fn is_self_or_descendant(
    conn: &Connection,
    record: RecordId,
    candidate: RecordId,
) -> Result<bool, SillokError> {
    let mut statement = match conn
        .prepare_cached("SELECT record_parent_record_id FROM record WHERE record_id = ?1")
    {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut current = Some(candidate);
    for _ in 0..MAX_DEPTH {
        let id = match current {
            Some(value) => value,
            None => return Ok(false),
        };
        if id == record {
            return Ok(true);
        }
        let parent: Option<Option<Vec<u8>>> = match statement
            .query_row([id.as_bytes().to_vec()], |row| row.get(0))
            .optional()
        {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        current = match parent {
            Some(Some(bytes)) => match RecordId::from_slice(&bytes) {
                Ok(value) => Some(value),
                Err(error) => return Err(error),
            },
            Some(None) | None => None,
        };
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::next_recorded_at;
    use crate::domain::time::Timestamp;

    #[test]
    fn recorded_at_stays_above_the_record() {
        let past = Timestamp::from_millis(1_000);
        let future = Timestamp::from_millis(Timestamp::now().as_millis() + 60_000);
        assert!(next_recorded_at(past, None) >= Timestamp::now().max(past));
        assert_eq!(
            next_recorded_at(past, Some(future)).as_millis(),
            future.as_millis() + 1
        );
    }
}
