//! Event log access.

use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::domain::event::context::WorkContext;
use crate::domain::event::envelope::Event;
use crate::domain::id::{EventId, RecordId};
use crate::domain::time::Timestamp;
use crate::error::SillokError;

/// One stored event line as sync sees it.
#[derive(Debug, Clone)]
pub struct StoredLine {
    pub event_id: EventId,
    pub recorded_at: Timestamp,
    pub raw: String,
}

/// Returns the id of a context row, inserting it if new.
pub fn context_id(conn: &Connection, context: &WorkContext) -> Result<i64, SillokError> {
    let json = match serde_json::to_string(context) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut statement = match conn.prepare_cached(
        "INSERT INTO work_context (work_context_json, work_context_cwd, work_context_git_root, work_context_session)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (work_context_json) DO UPDATE SET work_context_json = excluded.work_context_json
         RETURNING work_context_id",
    ) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    match statement.query_row(
        params![json, context.cwd, context.git_root, context.session],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(value),
        Err(error) => Err(error.into()),
    }
}

/// Inserts an event; returns false when its id is already stored.
pub fn insert(conn: &Connection, event: &Event) -> Result<bool, SillokError> {
    let context = match context_id(conn, &event.body.context) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut statement = match conn.prepare_cached(
        "INSERT INTO event (event_id, event_kind, event_record_id, event_occurred_at_ms,
            event_recorded_at_ms, event_work_context_id, event_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (event_id) DO NOTHING",
    ) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let record = event.body.kind.record_id().map(|id| id.as_bytes().to_vec());
    match statement.execute(params![
        event.id().as_bytes().to_vec(),
        event.body.kind.label(),
        record,
        event.body.event_at.as_millis(),
        event.body.recorded_at.as_millis(),
        context,
        event.raw,
    ]) {
        Ok(changed) => Ok(changed == 1),
        Err(error) => Err(error.into()),
    }
}

/// Replaces the stored bytes of an event (sync conflict convergence). Every
/// column derived from the JSON is rewritten so indexes match the new bytes.
pub fn replace(conn: &Connection, event: &Event) -> Result<(), SillokError> {
    let context = match context_id(conn, &event.body.context) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let record = event.body.kind.record_id().map(|id| id.as_bytes().to_vec());
    match conn.execute(
        "UPDATE event SET event_json = ?2, event_kind = ?3, event_record_id = ?4,
            event_occurred_at_ms = ?5, event_recorded_at_ms = ?6, event_work_context_id = ?7
         WHERE event_id = ?1",
        params![
            event.id().as_bytes().to_vec(),
            event.raw,
            event.body.kind.label(),
            record,
            event.body.event_at.as_millis(),
            event.body.recorded_at.as_millis(),
            context,
        ],
    ) {
        Ok(_) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Loads and parses every event in insertion order.
pub fn load_all(conn: &Connection) -> Result<Vec<Event>, SillokError> {
    load(conn, "SELECT event_json FROM event ORDER BY event_seq", &[])
}

/// Loads the events about one record, oldest first.
pub fn for_record(conn: &Connection, id: RecordId) -> Result<Vec<Event>, SillokError> {
    load(
        conn,
        "SELECT event_json FROM event WHERE event_record_id = ?1
         ORDER BY event_recorded_at_ms, event_id",
        &[&id.as_bytes().to_vec()],
    )
}

/// Loads id, recorded time, and raw bytes of every event, for sync diffs.
pub fn lines(conn: &Connection) -> Result<Vec<StoredLine>, SillokError> {
    let mut statement =
        match conn.prepare("SELECT event_id, event_recorded_at_ms, event_json FROM event") {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
    let rows = match statement.query_map([], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0),
            row.get::<_, i64>(1),
            row.get::<_, String>(2),
        ))
    }) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut out = Vec::new();
    for row in rows {
        match row {
            Ok((Ok(id), Ok(recorded), Ok(raw))) => match EventId::from_slice(&id) {
                Ok(event_id) => out.push(StoredLine {
                    event_id,
                    recorded_at: Timestamp::from_millis(recorded),
                    raw,
                }),
                Err(error) => return Err(error),
            },
            Ok(_) => {
                return Err(SillokError::datashape(
                    "invalid_datashape",
                    "event row has unexpected types",
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(out)
}

/// Activity labels per record for events occurring in `[start, end)`.
pub fn activity(
    conn: &Connection,
    start: Timestamp,
    end: Timestamp,
) -> Result<HashMap<RecordId, Vec<&'static str>>, SillokError> {
    let events = match load(
        conn,
        "SELECT event_json FROM event
         WHERE event_occurred_at_ms >= ?1 AND event_occurred_at_ms < ?2 AND event_record_id IS NOT NULL
         ORDER BY event_occurred_at_ms, event_seq",
        &[&start.as_millis(), &end.as_millis()],
    ) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut labels: HashMap<RecordId, Vec<&'static str>> = HashMap::new();
    for event in events {
        if let Some(id) = event.body.kind.record_id() {
            let entry = labels.entry(id).or_default();
            let label = event.body.kind.activity();
            if !entry.contains(&label) {
                entry.push(label);
            }
        }
    }
    Ok(labels)
}

/// Number of stored events.
pub fn count(conn: &Connection) -> Result<usize, SillokError> {
    match conn.query_row("SELECT COUNT(*) FROM event", [], |row| row.get::<_, i64>(0)) {
        Ok(value) => match usize::try_from(value) {
            Ok(count) => Ok(count),
            Err(error) => Err(SillokError::datashape(
                "invalid_datashape",
                error.to_string(),
            )),
        },
        Err(error) => Err(error.into()),
    }
}

fn load(
    conn: &Connection,
    sql: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Event>, SillokError> {
    let mut statement = match conn.prepare_cached(sql) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let rows = match statement.query_map(args, |row| row.get::<_, String>(0)) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut events = Vec::new();
    for row in rows {
        match row {
            Ok(raw) => match Event::parse(raw) {
                Ok(event) => events.push(event),
                Err(error) => return Err(error),
            },
            Err(error) => return Err(error.into()),
        }
    }
    Ok(events)
}

/// Streams raw event lines with `event_at` in the optional inclusive range,
/// in insertion order, without holding them all in memory.
pub fn stream(
    conn: &Connection,
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    sink: &mut dyn FnMut(&str) -> Result<(), SillokError>,
) -> Result<usize, SillokError> {
    let lower = match from {
        Some(value) => value.as_millis(),
        None => i64::MIN,
    };
    let upper = match to {
        Some(value) => value.as_millis(),
        None => i64::MAX,
    };
    let mut statement = match conn.prepare(
        "SELECT event_json FROM event
         WHERE event_occurred_at_ms >= ?1 AND event_occurred_at_ms <= ?2 ORDER BY event_seq",
    ) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let rows = match statement.query_map(params![lower, upper], |row| row.get::<_, String>(0)) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut count = 0usize;
    for row in rows {
        match row {
            Ok(raw) => match sink(&raw) {
                Ok(()) => count += 1,
                Err(error) => return Err(error),
            },
            Err(error) => return Err(error.into()),
        }
    }
    Ok(count)
}
