//! Read queries over the projection.

use std::collections::{HashMap, HashSet};

use rusqlite::ToSql;

use crate::domain::event::envelope::Event;
use crate::domain::id::RecordId;
use crate::domain::record::{Record, RecordKind, RecordStatus};
use crate::domain::time::Timestamp;
use crate::error::SillokError;
use crate::storage::sqlite::open::Store;
use crate::storage::sqlite::{events, records};

/// Activity labels per touched record.
pub type Activity = HashMap<RecordId, Vec<&'static str>>;

/// Longest parent chain walked when adding ancestors for context.
const MAX_ANCESTOR_DEPTH: usize = 64;

/// Filters for `query` and the listing commands.
#[derive(Debug, Clone, Default)]
pub struct RecordFilter {
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
    /// Every tag must be present.
    pub tags: Vec<String>,
    pub status: Option<RecordStatus>,
    pub kind: Option<RecordKind>,
    pub open_only: bool,
    /// Case-insensitive substring of the text.
    pub text: Option<String>,
    /// Substring of the repository root, else working directory.
    pub context: Option<String>,
    /// Exact repository root, else working directory, of the creating event.
    pub context_key: Option<String>,
    /// Newest records kept; output is still oldest first.
    pub limit: usize,
}

impl Store {
    /// One record by id.
    pub fn record(&self, id: RecordId) -> Result<Option<Record>, SillokError> {
        records::fetch(&self.conn, id)
    }

    /// Events about one record, oldest first.
    pub fn record_events(&self, id: RecordId) -> Result<Vec<Event>, SillokError> {
        events::for_record(&self.conn, id)
    }

    /// Filtered records, newest `limit` kept, returned oldest first.
    pub fn query(&self, filter: &RecordFilter) -> Result<Vec<Record>, SillokError> {
        let mut clauses: Vec<String> = Vec::new();
        let mut args: Vec<Box<dyn ToSql>> = Vec::new();
        match filter.status {
            Some(status) => bind(
                &mut clauses,
                &mut args,
                "r.record_status = #",
                Box::new(status.as_str()),
            ),
            None => clauses.push("r.record_status != 'retracted'".to_string()),
        }
        if filter.open_only {
            clauses.push("r.record_status IN ('open', 'active', 'blocked')".to_string());
        }
        if let Some(kind) = filter.kind {
            bind(
                &mut clauses,
                &mut args,
                "r.record_kind = #",
                Box::new(kind.as_str()),
            );
        }
        if let Some(from) = filter.from {
            bind(
                &mut clauses,
                &mut args,
                "r.record_created_at_ms >= #",
                Box::new(from.as_millis()),
            );
        }
        if let Some(to) = filter.to {
            bind(
                &mut clauses,
                &mut args,
                "r.record_created_at_ms <= #",
                Box::new(to.as_millis()),
            );
        }
        for tag in &filter.tags {
            bind(
                &mut clauses,
                &mut args,
                "EXISTS (SELECT 1 FROM record_tag t WHERE t.record_tag_record_id = r.record_id AND t.record_tag_text = #)",
                Box::new(tag.clone()),
            );
        }
        if let Some(text) = &filter.text {
            bind(
                &mut clauses,
                &mut args,
                "instr(lower(r.record_text), lower(#)) > 0",
                Box::new(text.clone()),
            );
        }
        if let Some(context) = &filter.context {
            bind(
                &mut clauses,
                &mut args,
                "instr(COALESCE(c.work_context_git_root, c.work_context_cwd, ''), #) > 0",
                Box::new(context.clone()),
            );
        }
        if let Some(key) = &filter.context_key {
            bind(
                &mut clauses,
                &mut args,
                "COALESCE(c.work_context_git_root, c.work_context_cwd) = #",
                Box::new(key.clone()),
            );
        }
        let limit = match i64::try_from(filter.limit) {
            Ok(value) => value,
            Err(_) => i64::MAX,
        };
        args.push(Box::new(limit));
        let tail = format!(
            "WHERE {} ORDER BY r.record_created_at_ms DESC, r.record_id DESC LIMIT ?{}",
            clauses.join(" AND "),
            args.len()
        );
        let refs: Vec<&dyn ToSql> = args.iter().map(|value| value.as_ref()).collect();
        match records::load(&self.conn, &tail, &refs) {
            Ok(mut found) => {
                found.reverse();
                Ok(found)
            }
            Err(error) => Err(error),
        }
    }

    /// Records by id, in no particular order.
    pub fn records(&self, ids: &[RecordId]) -> Result<Vec<Record>, SillokError> {
        records::fetch_many(&self.conn, ids)
    }

    /// Visible children of the given parents.
    pub fn children(&self, parents: &[RecordId]) -> Result<Vec<Record>, SillokError> {
        let mut out = Vec::new();
        for chunk in parents.chunks(500) {
            let blobs: Vec<Vec<u8>> = chunk.iter().map(|id| id.as_bytes().to_vec()).collect();
            let args: Vec<&dyn ToSql> = blobs.iter().map(|blob| blob as &dyn ToSql).collect();
            let tail = format!(
                "WHERE r.record_parent_record_id IN ({}) AND r.record_status != 'retracted'",
                records::placeholders(chunk.len())
            );
            match records::load(&self.conn, &tail, &args) {
                Ok(mut found) => out.append(&mut found),
                Err(error) => return Err(error),
            }
        }
        Ok(out)
    }

    /// A record and all visible descendants, one query per tree level.
    pub fn subtree(&self, root: Record) -> Result<Vec<Record>, SillokError> {
        let mut seen: HashSet<RecordId> = HashSet::from([root.id]);
        let mut frontier = vec![root.id];
        let mut all = vec![root];
        while !frontier.is_empty() {
            let level = match self.children(&frontier) {
                Ok(value) => value,
                Err(error) => return Err(error),
            };
            frontier = Vec::with_capacity(level.len());
            for record in level {
                if seen.insert(record.id) {
                    frontier.push(record.id);
                    all.push(record);
                }
            }
        }
        Ok(all)
    }

    /// Adds the visible ancestors of `found`, walking one level per query.
    /// A retracted ancestor stops the walk; its descendants show as roots.
    pub fn with_ancestors(&self, mut found: Vec<Record>) -> Result<Vec<Record>, SillokError> {
        let mut present: HashSet<RecordId> = found.iter().map(|record| record.id).collect();
        let mut wanted: Vec<RecordId> = found
            .iter()
            .filter_map(|record| record.parent)
            .filter(|parent| !present.contains(parent))
            .collect();
        for _ in 0..MAX_ANCESTOR_DEPTH {
            wanted.sort();
            wanted.dedup();
            if wanted.is_empty() {
                break;
            }
            let parents = match records::fetch_many(&self.conn, &wanted) {
                Ok(value) => value,
                Err(error) => return Err(error),
            };
            wanted = Vec::new();
            for parent in parents {
                if parent.status == RecordStatus::Retracted || !present.insert(parent.id) {
                    continue;
                }
                if let Some(next) = parent.parent
                    && !present.contains(&next)
                {
                    wanted.push(next);
                }
                found.push(parent);
            }
        }
        Ok(found)
    }

    /// Records with events in `[start, end)` plus their ancestors, and the
    /// activity labels of the touched ones.
    pub fn day(
        &self,
        start: Timestamp,
        end: Timestamp,
    ) -> Result<(Vec<Record>, Activity), SillokError> {
        let activity = match events::activity(&self.conn, start, end) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let ids: Vec<RecordId> = activity.keys().copied().collect();
        let touched = match records::fetch_many(&self.conn, &ids) {
            Ok(value) => value
                .into_iter()
                .filter(|record| record.status != RecordStatus::Retracted)
                .collect(),
            Err(error) => return Err(error),
        };
        match self.with_ancestors(touched) {
            Ok(all) => Ok((all, activity)),
            Err(error) => Err(error),
        }
    }
}

/// Appends a clause whose `#` becomes the next numbered parameter.
fn bind(
    clauses: &mut Vec<String>,
    args: &mut Vec<Box<dyn ToSql>>,
    template: &str,
    value: Box<dyn ToSql>,
) {
    args.push(value);
    clauses.push(template.replace('#', &format!("?{}", args.len())));
}
