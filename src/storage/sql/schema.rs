use turso::Connection;

use crate::error::SillokError;

pub const STORE_DATASHAPE_VERSION: i64 = 2;
pub const EVENT_DATASHAPE_VERSION: i64 = 2;
pub const RECORD_DATASHAPE_VERSION: i64 = 2;

/// Creates the v2 Turso schema and all query indexes.
pub async fn create(conn: &Connection) -> Result<(), SillokError> {
    conn.execute_batch(
        r#"
        PRAGMA user_version = 2;

        CREATE TABLE IF NOT EXISTS sillok_meta (
            meta_key TEXT PRIMARY KEY,
            meta_value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS work_contexts (
            context_id INTEGER PRIMARY KEY,
            context_json TEXT NOT NULL UNIQUE,
            context_cwd TEXT,
            context_git_root TEXT,
            context_git_branch TEXT,
            context_git_head TEXT,
            context_git_remote TEXT
        );

        CREATE TABLE IF NOT EXISTS days (
            day_id BLOB PRIMARY KEY,
            day_date TEXT NOT NULL,
            day_timezone TEXT NOT NULL,
            UNIQUE(day_date, day_timezone)
        );

        CREATE TABLE IF NOT EXISTS events (
            event_seq INTEGER PRIMARY KEY,
            event_id BLOB NOT NULL UNIQUE,
            event_datashape_version INTEGER NOT NULL,
            event_kind TEXT NOT NULL,
            event_primary_record_id BLOB,
            event_at_ms INTEGER NOT NULL,
            event_recorded_at_ms INTEGER NOT NULL,
            event_actor TEXT NOT NULL,
            event_context_id INTEGER NOT NULL,
            event_payload BLOB NOT NULL
        );

        CREATE TABLE IF NOT EXISTS event_refs (
            event_seq INTEGER NOT NULL,
            ref_record_id BLOB NOT NULL,
            ref_role TEXT NOT NULL,
            PRIMARY KEY(event_seq, ref_record_id, ref_role)
        );

        CREATE TABLE IF NOT EXISTS records (
            record_id BLOB PRIMARY KEY,
            record_datashape_version INTEGER NOT NULL,
            record_kind TEXT NOT NULL,
            record_day_id BLOB NOT NULL,
            record_parent_id BLOB,
            record_status TEXT NOT NULL,
            record_text TEXT NOT NULL,
            record_purpose TEXT,
            record_created_at_ms INTEGER NOT NULL,
            record_updated_at_ms INTEGER NOT NULL,
            record_context_id INTEGER NOT NULL,
            record_retraction_reason TEXT
        );

        CREATE TABLE IF NOT EXISTS record_tags (
            record_id BLOB NOT NULL,
            tag_text TEXT NOT NULL,
            PRIMARY KEY(record_id, tag_text)
        );

        CREATE INDEX IF NOT EXISTS idx_records_day
            ON records(record_day_id, record_created_at_ms, record_id);
        CREATE INDEX IF NOT EXISTS idx_records_parent
            ON records(record_parent_id, record_created_at_ms, record_id);
        CREATE INDEX IF NOT EXISTS idx_records_status
            ON records(record_status, record_created_at_ms, record_id);
        CREATE INDEX IF NOT EXISTS idx_records_timeline
            ON records(record_created_at_ms, record_id);
        CREATE INDEX IF NOT EXISTS idx_records_context
            ON records(record_context_id, record_created_at_ms, record_id);
        CREATE INDEX IF NOT EXISTS idx_record_tags_tag
            ON record_tags(tag_text, record_id);
        CREATE INDEX IF NOT EXISTS idx_events_recorded
            ON events(event_recorded_at_ms, event_seq);
        CREATE INDEX IF NOT EXISTS idx_events_at
            ON events(event_at_ms, event_seq);
        CREATE INDEX IF NOT EXISTS idx_event_refs_record
            ON event_refs(ref_record_id, event_seq);
        CREATE INDEX IF NOT EXISTS idx_days_key
            ON days(day_date, day_timezone);
        "#,
    )
    .await?;
    Ok(())
}
