//! Record projection rows.

use std::collections::HashMap;

use rusqlite::{Connection, Row, ToSql, params};

use crate::domain::event::context::WorkContext;
use crate::domain::id::RecordId;
use crate::domain::record::{Record, RecordKind, RecordStatus};
use crate::domain::time::Timestamp;
use crate::error::SillokError;
use crate::storage::sqlite::events::context_id;

/// Column list shared by every record query; `r` is `record`, `c` is `work_context`.
pub const RECORD_SELECT: &str = "SELECT r.record_id, r.record_kind, r.record_parent_record_id,
    r.record_status, r.record_prior_status, r.record_text, r.record_purpose, r.record_note,
    r.record_retraction_reason, r.record_created_at_ms, r.record_updated_at_ms, c.work_context_json
    FROM record r JOIN work_context c ON c.work_context_id = r.record_work_context_id";

/// SQLite bound-parameter budget per tag lookup.
const TAG_CHUNK: usize = 500;

/// Inserts or replaces one record and its tags.
pub fn upsert(conn: &Connection, record: &Record) -> Result<(), SillokError> {
    let context = match context_id(conn, &record.context) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let id = record.id.as_bytes().to_vec();
    let mut statement = match conn.prepare_cached(
        "INSERT INTO record (record_id, record_kind, record_parent_record_id, record_status,
            record_prior_status, record_text, record_purpose, record_note, record_retraction_reason,
            record_created_at_ms, record_updated_at_ms, record_work_context_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT (record_id) DO UPDATE SET
            record_parent_record_id = excluded.record_parent_record_id,
            record_status = excluded.record_status,
            record_prior_status = excluded.record_prior_status,
            record_text = excluded.record_text,
            record_purpose = excluded.record_purpose,
            record_note = excluded.record_note,
            record_retraction_reason = excluded.record_retraction_reason,
            record_updated_at_ms = excluded.record_updated_at_ms",
    ) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = statement.execute(params![
        id,
        record.kind.as_str(),
        record.parent.map(|parent| parent.as_bytes().to_vec()),
        record.status.as_str(),
        record.prior_status.map(RecordStatus::as_str),
        record.text,
        record.purpose,
        record.note,
        record.retraction_reason,
        record.created_at.as_millis(),
        record.updated_at.as_millis(),
        context,
    ]) {
        return Err(error.into());
    }
    replace_tags(conn, record)
}

fn replace_tags(conn: &Connection, record: &Record) -> Result<(), SillokError> {
    let id = record.id.as_bytes().to_vec();
    let mut delete =
        match conn.prepare_cached("DELETE FROM record_tag WHERE record_tag_record_id = ?1") {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
    if let Err(error) = delete.execute([&id]) {
        return Err(error.into());
    }
    let mut insert = match conn.prepare_cached(
        "INSERT INTO record_tag (record_tag_record_id, record_tag_text) VALUES (?1, ?2)",
    ) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    for tag in &record.tags {
        if let Err(error) = insert.execute(params![id, tag]) {
            return Err(error.into());
        }
    }
    Ok(())
}

/// Runs a record query and attaches tags. `tail` is appended after the
/// shared select (joins, WHERE, ORDER BY, LIMIT) and must bind only `args`.
pub fn load(
    conn: &Connection,
    tail: &str,
    args: &[&dyn ToSql],
) -> Result<Vec<Record>, SillokError> {
    let sql = format!("{RECORD_SELECT} {tail}");
    let mut statement = match conn.prepare_cached(&sql) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let rows = match statement.query_map(args, map_row) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut records = Vec::new();
    for row in rows {
        match row {
            Ok(Ok(record)) => records.push(record),
            Ok(Err(error)) => return Err(error),
            Err(error) => return Err(error.into()),
        }
    }
    match attach_tags(conn, &mut records) {
        Ok(()) => Ok(records),
        Err(error) => Err(error),
    }
}

/// Loads one record.
pub fn fetch(conn: &Connection, id: RecordId) -> Result<Option<Record>, SillokError> {
    match load(conn, "WHERE r.record_id = ?1", &[&id.as_bytes().to_vec()]) {
        Ok(mut records) => Ok(records.pop()),
        Err(error) => Err(error),
    }
}

/// Loads records by id, in no particular order.
pub fn fetch_many(conn: &Connection, ids: &[RecordId]) -> Result<Vec<Record>, SillokError> {
    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(TAG_CHUNK) {
        let placeholders = placeholders(chunk.len());
        let blobs: Vec<Vec<u8>> = chunk.iter().map(|id| id.as_bytes().to_vec()).collect();
        let args: Vec<&dyn ToSql> = blobs.iter().map(|blob| blob as &dyn ToSql).collect();
        match load(
            conn,
            &format!("WHERE r.record_id IN ({placeholders})"),
            &args,
        ) {
            Ok(mut records) => out.append(&mut records),
            Err(error) => return Err(error),
        }
    }
    Ok(out)
}

fn attach_tags(conn: &Connection, records: &mut [Record]) -> Result<(), SillokError> {
    if records.is_empty() {
        return Ok(());
    }
    let mut tags: HashMap<RecordId, Vec<String>> = HashMap::with_capacity(records.len());
    for chunk in records.chunks(TAG_CHUNK) {
        let sql = format!(
            "SELECT record_tag_record_id, record_tag_text FROM record_tag
             WHERE record_tag_record_id IN ({}) ORDER BY record_tag_text",
            placeholders(chunk.len())
        );
        let blobs: Vec<Vec<u8>> = chunk
            .iter()
            .map(|record| record.id.as_bytes().to_vec())
            .collect();
        let args: Vec<&dyn ToSql> = blobs.iter().map(|blob| blob as &dyn ToSql).collect();
        let mut statement = match conn.prepare_cached(&sql) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let rows = match statement.query_map(args.as_slice(), |row| {
            Ok((row.get::<_, Vec<u8>>(0), row.get::<_, String>(1)))
        }) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        for row in rows {
            match row {
                Ok((Ok(id), Ok(tag))) => match RecordId::from_slice(&id) {
                    Ok(record_id) => tags.entry(record_id).or_default().push(tag),
                    Err(error) => return Err(error),
                },
                Ok(_) => {
                    return Err(SillokError::datashape(
                        "invalid_datashape",
                        "tag row has unexpected types",
                    ));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    for record in records {
        if let Some(values) = tags.remove(&record.id) {
            record.tags = values;
        }
    }
    Ok(())
}

/// `?1, ?2, ...` for an IN list.
pub fn placeholders(count: usize) -> String {
    (1..=count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Maps one row of `RECORD_SELECT`; the domain result rides inside the
/// rusqlite result so conversion failures keep their own error codes.
fn map_row(row: &Row<'_>) -> rusqlite::Result<Result<Record, SillokError>> {
    Ok(read_record(row))
}

fn column<T: rusqlite::types::FromSql>(row: &Row<'_>, index: usize) -> Result<T, SillokError> {
    match row.get::<_, T>(index) {
        Ok(value) => Ok(value),
        Err(error) => Err(error.into()),
    }
}

fn read_record(row: &Row<'_>) -> Result<Record, SillokError> {
    let id = match column::<Vec<u8>>(row, 0) {
        Ok(bytes) => match RecordId::from_slice(&bytes) {
            Ok(value) => value,
            Err(error) => return Err(error),
        },
        Err(error) => return Err(error),
    };
    let kind = match column::<String>(row, 1) {
        Ok(raw) => match RecordKind::parse(&raw) {
            Ok(value) => value,
            Err(error) => return Err(error),
        },
        Err(error) => return Err(error),
    };
    let parent = match column::<Option<Vec<u8>>>(row, 2) {
        Ok(Some(bytes)) => match RecordId::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(error) => return Err(error),
        },
        Ok(None) => None,
        Err(error) => return Err(error),
    };
    let status = match column::<String>(row, 3) {
        Ok(raw) => match RecordStatus::parse(&raw) {
            Ok(value) => value,
            Err(error) => return Err(error),
        },
        Err(error) => return Err(error),
    };
    let prior_status = match column::<Option<String>>(row, 4) {
        Ok(Some(raw)) => match RecordStatus::parse(&raw) {
            Ok(value) => Some(value),
            Err(error) => return Err(error),
        },
        Ok(None) => None,
        Err(error) => return Err(error),
    };
    let context = match column::<String>(row, 11) {
        Ok(raw) => match serde_json::from_str::<WorkContext>(&raw) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        },
        Err(error) => return Err(error),
    };
    match (
        column::<String>(row, 5),
        column::<Option<String>>(row, 6),
        column::<Option<String>>(row, 7),
        column::<Option<String>>(row, 8),
        column::<i64>(row, 9),
        column::<i64>(row, 10),
    ) {
        (Ok(text), Ok(purpose), Ok(note), Ok(reason), Ok(created), Ok(updated)) => Ok(Record {
            id,
            kind,
            parent,
            status,
            text,
            purpose,
            note,
            tags: Vec::new(),
            retraction_reason: reason,
            prior_status,
            created_at: Timestamp::from_millis(created),
            updated_at: Timestamp::from_millis(updated),
            context,
        }),
        (Err(error), ..)
        | (_, Err(error), ..)
        | (_, _, Err(error), ..)
        | (_, _, _, Err(error), ..)
        | (_, _, _, _, Err(error), _)
        | (.., Err(error)) => Err(error),
    }
}
