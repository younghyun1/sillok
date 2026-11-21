//! Recently touched records, for `status`.

use crate::domain::event::context::{Scope, remote_key};
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
/// outside Git, or in any checkout of the same remote. Binds the values of
/// `scope_params` starting at parameter `first`.
pub fn scope_clause(first: usize, scope: &Scope) -> String {
    let mut clause = format!(
        "(c.work_context_git_root IN (?{root}, ?{main})
          OR (c.work_context_git_root IS NULL AND c.work_context_cwd = ?{root})",
        root = first,
        main = first + 1
    );
    if !scope.remotes.is_empty() {
        let marks: Vec<String> = (0..scope.remotes.len())
            .map(|offset| format!("?{}", first + 2 + offset))
            .collect();
        clause.push_str(&format!(
            " OR json_extract(c.work_context_json, '$.git_remote') IN ({})",
            marks.join(", ")
        ));
    }
    clause.push(')');
    clause
}

/// Values for `scope_clause`: root, main checkout (the root again when there
/// is none), then each remote spelling.
pub fn scope_params(scope: &Scope) -> Vec<String> {
    let main = match &scope.main_root {
        Some(value) => value.clone(),
        None => scope.root.clone(),
    };
    let mut params = Vec::with_capacity(2 + scope.remotes.len());
    params.push(scope.root.clone());
    params.push(main);
    params.extend(scope.remotes.iter().cloned());
    params
}

impl Store {
    /// Every stored spelling of the remote `url` names, plus `url` itself.
    ///
    /// Walks the distinct values of the remote index with one seek each (a
    /// loose index scan), so the cost follows the number of repositories,
    /// not the number of contexts.
    pub fn remote_spellings(&self, url: &str) -> Result<Vec<String>, SillokError> {
        let mut statement = match self.conn.prepare_cached(
            "WITH RECURSIVE remote(url) AS (
                SELECT MIN(json_extract(work_context_json, '$.git_remote')) FROM work_context
                UNION ALL
                SELECT (SELECT MIN(json_extract(work_context_json, '$.git_remote'))
                        FROM work_context
                        WHERE json_extract(work_context_json, '$.git_remote') > remote.url)
                FROM remote WHERE remote.url IS NOT NULL
             )
             SELECT url FROM remote WHERE url IS NOT NULL",
        ) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let rows = match statement.query_map([], |row| row.get::<_, String>(0)) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let key = remote_key(url);
        let mut spellings = vec![url.to_string()];
        for row in rows {
            match row {
                Ok(stored) => {
                    if stored != url && remote_key(&stored) == key {
                        spellings.push(stored);
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(spellings)
    }
}

fn recent_ids_in_scope(
    store: &Store,
    scope: &Scope,
    limit: i64,
) -> Result<Vec<RecordId>, SillokError> {
    let values = scope_params(scope);
    let sql = format!(
        "SELECT e.event_record_id FROM event e
         JOIN work_context c ON c.work_context_id = e.event_work_context_id
         WHERE e.event_record_id IS NOT NULL AND {}
         GROUP BY e.event_record_id
         ORDER BY MAX(e.event_recorded_at_ms) DESC
         LIMIT ?{}",
        scope_clause(1, scope),
        values.len() + 1
    );
    let mut statement = match store.conn.prepare_cached(&sql) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut args: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(values.len() + 1);
    for value in &values {
        args.push(value);
    }
    args.push(&limit);
    let rows = match statement.query_map(args.as_slice(), |row| row.get::<_, Vec<u8>>(0)) {
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
