//! Store schema v3.
//!
//! `event` is the authoritative log; `event_json` keeps each event's
//! canonical bytes, a deliberate exception to normalization because those
//! bytes are what sync compares and what future versions extend. Every other
//! table is a projection that `rebuild` can recreate from `event`.
//!
//! Text bounds in the CHECK constraints mirror `domain::text`.

use rusqlite::Connection;

use crate::error::SillokError;

/// Value of `store_meta.store_version` for this layout.
pub const STORE_VERSION: &str = "3";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS store_meta (
    store_meta_key TEXT NOT NULL CONSTRAINT store_meta_pk PRIMARY KEY,
    store_meta_value TEXT NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS work_context (
    work_context_id INTEGER NOT NULL CONSTRAINT work_context_pk PRIMARY KEY,
    work_context_json TEXT NOT NULL CONSTRAINT work_context_json_unique UNIQUE,
    work_context_cwd TEXT,
    work_context_git_root TEXT,
    work_context_session TEXT
) STRICT;
CREATE INDEX IF NOT EXISTS work_context_git_root_idx ON work_context (work_context_git_root);
CREATE INDEX IF NOT EXISTS work_context_cwd_idx ON work_context (work_context_cwd);

CREATE TABLE IF NOT EXISTS event (
    event_seq INTEGER NOT NULL CONSTRAINT event_pk PRIMARY KEY,
    event_id BLOB NOT NULL CONSTRAINT event_id_unique UNIQUE
        CONSTRAINT event_id_length CHECK (length(event_id) = 16),
    event_kind TEXT NOT NULL,
    event_record_id BLOB
        CONSTRAINT event_record_id_length CHECK (event_record_id IS NULL OR length(event_record_id) = 16),
    event_occurred_at_ms INTEGER NOT NULL,
    event_recorded_at_ms INTEGER NOT NULL,
    event_work_context_id INTEGER NOT NULL
        CONSTRAINT event_work_context_fk REFERENCES work_context (work_context_id),
    event_json TEXT NOT NULL
        CONSTRAINT event_json_length CHECK (length(event_json) BETWEEN 2 AND 1048576)
) STRICT;
CREATE INDEX IF NOT EXISTS event_occurred_idx ON event (event_occurred_at_ms, event_record_id);
CREATE INDEX IF NOT EXISTS event_record_idx ON event (event_record_id, event_occurred_at_ms);
CREATE INDEX IF NOT EXISTS event_recorded_idx ON event (event_recorded_at_ms, event_id);
CREATE INDEX IF NOT EXISTS event_work_context_idx ON event (event_work_context_id, event_recorded_at_ms);

CREATE TABLE IF NOT EXISTS record (
    record_id BLOB NOT NULL CONSTRAINT record_pk PRIMARY KEY
        CONSTRAINT record_id_length CHECK (length(record_id) = 16),
    record_kind TEXT NOT NULL
        CONSTRAINT record_kind_check CHECK (record_kind IN ('objective', 'task')),
    record_parent_record_id BLOB
        CONSTRAINT record_parent_fk REFERENCES record (record_id) DEFERRABLE INITIALLY DEFERRED,
    record_status TEXT NOT NULL
        CONSTRAINT record_status_check CHECK (record_status IN ('open', 'active', 'blocked', 'completed', 'retracted')),
    record_prior_status TEXT
        CONSTRAINT record_prior_status_check CHECK (record_prior_status IS NULL OR record_prior_status IN ('open', 'active', 'blocked', 'completed')),
    record_text TEXT NOT NULL
        CONSTRAINT record_text_length CHECK (length(record_text) BETWEEN 1 AND 4096),
    record_purpose TEXT
        CONSTRAINT record_purpose_length CHECK (record_purpose IS NULL OR length(record_purpose) BETWEEN 1 AND 2048),
    record_note TEXT
        CONSTRAINT record_note_length CHECK (record_note IS NULL OR length(record_note) BETWEEN 1 AND 2048),
    record_retraction_reason TEXT
        CONSTRAINT record_retraction_reason_length CHECK (record_retraction_reason IS NULL OR length(record_retraction_reason) BETWEEN 1 AND 2048),
    record_created_at_ms INTEGER NOT NULL,
    record_updated_at_ms INTEGER NOT NULL,
    record_work_context_id INTEGER NOT NULL
        CONSTRAINT record_work_context_fk REFERENCES work_context (work_context_id),
    CONSTRAINT record_retraction_pair CHECK ((record_status = 'retracted') = (record_retraction_reason IS NOT NULL))
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS record_parent_idx ON record (record_parent_record_id, record_created_at_ms);
CREATE INDEX IF NOT EXISTS record_kind_status_idx ON record (record_kind, record_status, record_created_at_ms);
CREATE INDEX IF NOT EXISTS record_created_idx ON record (record_created_at_ms, record_id);
CREATE INDEX IF NOT EXISTS record_updated_idx ON record (record_updated_at_ms);
CREATE INDEX IF NOT EXISTS record_work_context_idx ON record (record_work_context_id);

CREATE TABLE IF NOT EXISTS record_tag (
    record_tag_record_id BLOB NOT NULL
        CONSTRAINT record_tag_record_fk REFERENCES record (record_id) ON DELETE CASCADE,
    record_tag_text TEXT NOT NULL
        CONSTRAINT record_tag_text_length CHECK (length(record_tag_text) BETWEEN 1 AND 96),
    CONSTRAINT record_tag_pk PRIMARY KEY (record_tag_record_id, record_tag_text)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS record_tag_text_idx ON record_tag (record_tag_text, record_tag_record_id);
";

/// Creates every table and index; idempotent.
pub fn create(conn: &Connection) -> Result<(), SillokError> {
    match conn.execute_batch(SCHEMA) {
        Ok(()) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};

    use super::create;
    use crate::domain::text::{DETAIL_MAX_CHARS, ENTRY_MAX_CHARS, TAG_MAX_CHARS};
    use crate::error::SillokError;

    fn insert_record(conn: &Connection, text: &str) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO record (record_id, record_kind, record_status, record_text,
                record_created_at_ms, record_updated_at_ms, record_work_context_id)
             VALUES (randomblob(16), 'task', 'open', ?1, 0, 0, 1)",
            params![text],
        )
    }

    #[test]
    fn text_checks_match_domain_bounds() -> Result<(), SillokError> {
        let conn = match Connection::open_in_memory() {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = create(&conn) {
            return Err(error);
        }
        if let Err(error) = conn.execute(
            "INSERT INTO work_context (work_context_id, work_context_json) VALUES (1, '{}')",
            [],
        ) {
            return Err(error.into());
        }
        // Character counts, not bytes: Hangul is three bytes per character.
        assert!(insert_record(&conn, &"가".repeat(ENTRY_MAX_CHARS)).is_ok());
        assert!(insert_record(&conn, &"가".repeat(ENTRY_MAX_CHARS + 1)).is_err());
        assert!(insert_record(&conn, "").is_err());
        assert_eq!(DETAIL_MAX_CHARS, 2048);
        assert_eq!(TAG_MAX_CHARS, 96);
        Ok(())
    }
}
