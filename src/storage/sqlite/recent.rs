//! Recently touched records, for `status`.

use crate::domain::event::context::Scope;
use crate::domain::id::RecordId;
use crate::domain::record::{Record, RecordStatus};
use crate::error::SillokError;
use crate::storage::sqlite::open::Store;
use crate::storage::sqlite::records;

impl Store {
    /// Most recently touched visible records, optionally in one repository.
    pub fn recent(&self, scope: Option<&Scope>, limit: usize) -> Result<Vec<Record>, SillokError> {
        let limit = match i64::try_from(limit) {
            Ok(value) => value,
            Err(_) => i64::MAX,
        };
        let ids = match scope {
            Some(scope) => match recent_ids_in_scope(self, scope, limit) {
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

/// SQL predicate selecting the `work_context` rows (alias `c`) of a scope:
/// recorded under its root or its main checkout's, in the same directory
/// outside Git, or in any checkout of the same remote. Binds the three
/// values of `scope_params` starting at parameter `first`.
pub fn scope_clause(first: usize) -> String {
    format!(
        "(c.work_context_git_root IN (?{root}, ?{main})
          OR (c.work_context_git_root IS NULL AND c.work_context_cwd = ?{root})
          OR json_extract(c.work_context_json, '$.git_remote') = ?{remote})",
        root = first,
        main = first + 1,
        remote = first + 2
    )
}

/// Values for `scope_clause`. A missing main checkout repeats the root and a
/// missing remote binds NULL, which matches nothing.
pub fn scope_params(scope: &Scope) -> (&str, &str, Option<&str>) {
    let main = match &scope.main_root {
        Some(value) => value.as_str(),
        None => scope.root.as_str(),
    };
    (scope.root.as_str(), main, scope.remote.as_deref())
}

fn recent_ids_in_scope(
    store: &Store,
    scope: &Scope,
    limit: i64,
) -> Result<Vec<RecordId>, SillokError> {
    let sql = format!(
        "SELECT e.event_record_id FROM event e
         JOIN work_context c ON c.work_context_id = e.event_work_context_id
         WHERE e.event_record_id IS NOT NULL AND {}
         GROUP BY e.event_record_id
         ORDER BY MAX(e.event_recorded_at_ms) DESC
         LIMIT ?4",
        scope_clause(1)
    );
    let mut statement = match store.conn.prepare_cached(&sql) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let (root, main, remote) = scope_params(scope);
    let rows = match statement.query_map(rusqlite::params![root, main, remote, limit], |row| {
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
