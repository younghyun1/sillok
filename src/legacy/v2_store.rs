//! Reader for 0.9/0.10 SQLite stores (datashape v2, written by Turso).
//!
//! Turso writes standard SQLite database and WAL files, so bundled SQLite
//! reads them directly, including events still sitting in the WAL.

use rusqlite::{Connection, OptionalExtension};

use crate::error::SillokError;
use crate::legacy::types::{
    LegacyArchive, LegacyEvent, LegacyEventKind, LegacyId, LegacyTimestamp, LegacyWorkContext,
};

/// The only v2 datashape 0.9 and 0.10 wrote.
const V2_VERSION: &str = "2";

/// Whether the connection holds a 0.9/0.10 store.
pub fn is_v2_store(conn: &Connection) -> Result<bool, SillokError> {
    let table: Option<String> = match conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'sillok_meta'",
            [],
            |row| row.get(0),
        )
        .optional()
    {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    Ok(table.is_some())
}

/// Reads every event of a v2 store in its original order.
pub fn read_v2_store(conn: &Connection) -> Result<LegacyArchive, SillokError> {
    let version = match meta(conn, "store_datashape_version") {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if version != V2_VERSION {
        return Err(SillokError::datashape(
            "unsupported_datashape",
            format!("store datashape `{version}` is not a 0.9/0.10 store"),
        ));
    }
    let archive_id = match meta(conn, "archive_id") {
        Ok(raw) => match uuid::Uuid::parse_str(&raw) {
            Ok(value) => LegacyId(*value.as_bytes()),
            Err(error) => {
                return Err(SillokError::datashape(
                    "invalid_datashape",
                    error.to_string(),
                ));
            }
        },
        Err(error) => return Err(error),
    };
    let created_at = match meta(conn, "created_at_ms") {
        Ok(raw) => match raw.parse::<i64>() {
            Ok(value) => LegacyTimestamp(value),
            Err(error) => {
                return Err(SillokError::datashape(
                    "invalid_datashape",
                    error.to_string(),
                ));
            }
        },
        Err(error) => return Err(error),
    };
    let mut statement = match conn.prepare(
        "SELECT e.event_id, e.event_datashape_version, e.event_at_ms, e.event_recorded_at_ms,
                e.event_actor, e.event_payload, c.context_cwd, c.context_git_root,
                c.context_git_branch, c.context_git_head, c.context_git_remote
         FROM events e JOIN work_contexts c ON c.context_id = e.event_context_id
         ORDER BY e.event_seq",
    ) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut rows = match statement.query([]) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut events = Vec::new();
    loop {
        let row = match rows.next() {
            Ok(Some(value)) => value,
            Ok(None) => break,
            Err(error) => return Err(error.into()),
        };
        match row_event(row) {
            Ok(event) => events.push(event),
            Err(error) => return Err(error),
        }
    }
    Ok(LegacyArchive {
        schema_version: 1,
        archive_id,
        created_at,
        events,
    })
}

fn row_event(row: &rusqlite::Row<'_>) -> Result<LegacyEvent, SillokError> {
    let id_bytes: Vec<u8> = match row.get(0) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let event_id = match <[u8; 16]>::try_from(id_bytes.as_slice()) {
        Ok(value) => LegacyId(value),
        Err(_) => {
            return Err(SillokError::datashape(
                "invalid_datashape",
                "v2 event id is not 16 bytes",
            ));
        }
    };
    let version: i64 = match row.get(1) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    if version != 2 {
        return Err(SillokError::datashape(
            "unsupported_datashape",
            format!("v2 event datashape `{version}` is not supported"),
        ));
    }
    let payload: Vec<u8> = match row.get(5) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let kind = match bitcode::decode::<LegacyEventKind>(&payload) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let values = (
        row.get::<_, i64>(2),
        row.get::<_, i64>(3),
        row.get::<_, String>(4),
        row.get::<_, Option<String>>(6),
        row.get::<_, Option<String>>(7),
        row.get::<_, Option<String>>(8),
        row.get::<_, Option<String>>(9),
        row.get::<_, Option<String>>(10),
    );
    match values {
        (Ok(at), Ok(recorded), Ok(actor), Ok(cwd), Ok(root), Ok(branch), Ok(head), Ok(remote)) => {
            Ok(LegacyEvent {
                event_id,
                event_at: LegacyTimestamp(at),
                recorded_at: LegacyTimestamp(recorded),
                actor,
                context: LegacyWorkContext {
                    cwd,
                    git_root: root,
                    git_branch: branch,
                    git_head: head,
                    git_remote: remote,
                },
                kind,
            })
        }
        _ => Err(SillokError::datashape(
            "invalid_datashape",
            "v2 event row has unexpected column types",
        )),
    }
}

fn meta(conn: &Connection, key: &str) -> Result<String, SillokError> {
    match conn
        .query_row(
            "SELECT meta_value FROM sillok_meta WHERE meta_key = ?1",
            [key],
            |row| row.get::<_, String>(0),
        )
        .optional()
    {
        Ok(Some(value)) => Ok(value),
        Ok(None) => Err(SillokError::datashape(
            "invalid_datashape",
            format!("v2 store is missing metadata `{key}`"),
        )),
        Err(error) => Err(error.into()),
    }
}
