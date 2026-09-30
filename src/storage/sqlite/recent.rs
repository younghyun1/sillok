//! Recently touched records, for `status`.

use crate::domain::id::RecordId;
use crate::domain::record::{Record, RecordStatus};
use crate::error::SillokError;
use crate::storage::sqlite::open::Store;
use crate::storage::sqlite::records;

impl Store {
    /// Most recently touched visible records, optionally in one context.
    pub fn recent(&self, context: Option<&str>, limit: usize) -> Result<Vec<Record>, SillokError> {
        let limit = match i64::try_from(limit) {
            Ok(value) => value,
            Err(_) => i64::MAX,
        };
        let ids = match context {
            Some(key) => match recent_ids_in_context(self, key, limit) {
                Ok(value) => value,
                Err(error) => return Err(error),
            },
            None => {
                let tail = "WHERE r.record_status != 'retracted' ORDER BY r.record_updated_at_ms DESC LIMIT ?1";
                return records::load(&self.conn, tail, &[&limit]);
            }
        };
        match records::fetch_many(&self.conn, &ids) {
            Ok(mut found) => {
                found.retain(|record| record.status != RecordStatus::Retracted);
                found.sort_by_key(|record| std::cmp::Reverse(record.updated_at));
                Ok(found)
            }
            Err(error) => Err(error),
        }
    }
}

fn recent_ids_in_context(
    store: &Store,
    key: &str,
    limit: i64,
) -> Result<Vec<RecordId>, SillokError> {
    let mut statement = match store.conn.prepare_cached(
        "SELECT e.event_record_id FROM event e
         JOIN work_context c ON c.work_context_id = e.event_work_context_id
         WHERE e.event_record_id IS NOT NULL
           AND (c.work_context_git_root = ?1 OR (c.work_context_git_root IS NULL AND c.work_context_cwd = ?1))
         GROUP BY e.event_record_id
         ORDER BY MAX(e.event_recorded_at_ms) DESC
         LIMIT ?2",
    ) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let rows = match statement.query_map(rusqlite::params![key, limit], |row| {
        row.get::<_, Vec<u8>>(0)
    }) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut ids = Vec::new();
    for row in rows {
        match row {
            Ok(bytes) => match RecordId::from_slice(&bytes) {
                Ok(id) => ids.push(id),
                Err(error) => return Err(error),
            },
            Err(error) => return Err(error.into()),
        }
    }
    Ok(ids)
}
