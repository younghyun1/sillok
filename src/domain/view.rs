use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use crate::domain::archive::{ARCHIVE_SCHEMA_VERSION, Archive};
use crate::domain::event::{ChronicleEvent, RecordKind, RecordStatus, WorkContext};
use crate::domain::id::ChronicleId;
use crate::domain::time::{DayKey, Timestamp};
use crate::error::SillokError;

/// Current record state derived from append-only events.
#[derive(Debug, Clone, Serialize)]
pub struct DerivedRecord {
    pub record_id: ChronicleId,
    pub kind: RecordKind,
    pub day_id: ChronicleId,
    pub parent_id: Option<ChronicleId>,
    pub text: String,
    pub purpose: Option<String>,
    pub tags: Vec<String>,
    pub status: RecordStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub context: WorkContext,
    pub retraction_reason: Option<String>,
    pub day_key: Option<DayKey>,
}

/// Tree output node used by `tree` and day summaries.
#[derive(Debug, Clone, Serialize)]
pub struct RecordTreeNode {
    pub record: DerivedRecord,
    pub children: Vec<RecordTreeNode>,
}

/// In-memory projection optimized for command execution.
#[derive(Debug)]
pub struct ChronicleView<'a> {
    pub archive: &'a Archive,
    pub records: HashMap<ChronicleId, DerivedRecord>,
    pub children: HashMap<ChronicleId, Vec<ChronicleId>>,
    pub parent_by_child: HashMap<ChronicleId, ChronicleId>,
    pub day_by_key: HashMap<DayKey, ChronicleId>,
    pub by_day: HashMap<ChronicleId, Vec<ChronicleId>>,
    pub timeline: BTreeMap<Timestamp, Vec<ChronicleId>>,
    pub by_tag: HashMap<String, Vec<ChronicleId>>,
    pub by_context: HashMap<String, Vec<ChronicleId>>,
    pub by_status: HashMap<RecordStatus, Vec<ChronicleId>>,
}

#[derive(Debug, Clone, Copy)]
struct RecordQuery<'a> {
    from: Timestamp,
    to: Timestamp,
    context: Option<&'a str>,
    tag: Option<&'a str>,
    status: Option<RecordStatus>,
}

impl<'a> ChronicleView<'a> {
    /// Builds the derived state and secondary indexes from an archive.
    pub fn build(archive: &'a Archive) -> Result<Self, SillokError> {
        if archive.schema_version != ARCHIVE_SCHEMA_VERSION {
            return Err(SillokError::new(
                "unsupported_schema",
                format!("archive schema {} is not supported", archive.schema_version),
            ));
        }

        let event_count = archive.events.len();
        let mut view = Self {
            archive,
            records: HashMap::with_capacity(event_count),
            children: HashMap::with_capacity(event_count.saturating_div(2)),
            parent_by_child: HashMap::with_capacity(event_count.saturating_div(2)),
            day_by_key: HashMap::with_capacity(event_count.saturating_div(8)),
            by_day: HashMap::with_capacity(event_count.saturating_div(2)),
            timeline: BTreeMap::new(),
            by_tag: HashMap::with_capacity(event_count.saturating_div(2)),
            by_context: HashMap::with_capacity(event_count.saturating_div(4)),
            by_status: HashMap::with_capacity(RecordStatus::variant_count()),
        };

        for event in &archive.events {
            crate::domain::reducer::apply_event(&mut view, event)?;
        }
        crate::domain::reducer::rebuild_indexes(&mut view);
        crate::domain::reducer::validate_parent_graph(&view)?;
        Ok(view)
    }

    /// Returns a record by id.
    pub fn record(&self, id: &ChronicleId) -> Option<&DerivedRecord> {
        self.records.get(id)
    }

    /// Returns a day id by key.
    pub fn day_id(&self, key: &DayKey) -> Option<ChronicleId> {
        self.day_by_key.get(key).copied()
    }

    /// Returns current visible records ordered by creation time and id.
    pub fn visible_records(&self) -> Vec<DerivedRecord> {
        let mut records = Vec::with_capacity(self.records.len());
        for bucket in self.timeline.values() {
            for id in bucket {
                let Some(record) = self.records.get(id) else {
                    continue;
                };
                if record.status != RecordStatus::Retracted {
                    records.push(record.clone());
                }
            }
        }
        records
    }

    /// Returns visible records within an inclusive creation-time range.
    pub fn query(
        &self,
        from: Timestamp,
        to: Timestamp,
        context: Option<&str>,
        tag: Option<&str>,
        status: Option<RecordStatus>,
    ) -> Vec<DerivedRecord> {
        let mut records = Vec::new();
        let filter = RecordQuery {
            from,
            to,
            context,
            tag,
            status,
        };
        match (filter.tag, filter.status) {
            (Some(required_tag), Some(required_status)) => {
                let Some(tag_ids) = self.by_tag.get(required_tag) else {
                    return records;
                };
                let Some(status_ids) = self.by_status.get(&required_status) else {
                    return records;
                };
                if tag_ids.len() <= status_ids.len() {
                    self.push_matching_records(tag_ids, &mut records, filter);
                } else {
                    self.push_matching_records(status_ids, &mut records, filter);
                }
            }
            (Some(required_tag), None) => {
                let Some(ids) = self.by_tag.get(required_tag) else {
                    return records;
                };
                self.push_matching_records(ids, &mut records, filter);
            }
            (None, Some(required_status)) => {
                let Some(ids) = self.by_status.get(&required_status) else {
                    return records;
                };
                self.push_matching_records(ids, &mut records, filter);
            }
            (None, None) => {
                for (_timestamp, bucket) in self.timeline.range(filter.from..=filter.to) {
                    self.push_matching_records(bucket, &mut records, filter);
                }
            }
        }
        sort_records(&mut records);
        records
    }

    /// Returns visible non-day records for a day ordered by creation time and id.
    pub fn records_for_day(&self, day_id: ChronicleId) -> Vec<DerivedRecord> {
        let Some(ids) = self.by_day.get(&day_id) else {
            return Vec::new();
        };
        let mut records = Vec::with_capacity(ids.len().saturating_sub(1));
        for id in ids {
            let Some(record) = self.records.get(id) else {
                continue;
            };
            if record.record_id != day_id && record.status != RecordStatus::Retracted {
                records.push(record.clone());
            }
        }
        records
    }

    /// Returns the event history affecting one record.
    pub fn events_for_record(&self, id: ChronicleId) -> Vec<ChronicleEvent> {
        let mut events = Vec::new();
        for event in self.archive.events.iter() {
            if event.references(id) {
                events.push(event.clone());
            }
        }
        events.sort_by_key(|event| (event.recorded_at, event.event_id));
        events
    }

    /// Builds a visible tree from a root id.
    pub fn tree(&self, root_id: ChronicleId) -> Result<RecordTreeNode, SillokError> {
        let Some(root) = self.records.get(&root_id) else {
            return Err(SillokError::new(
                "record_not_found",
                format!("record `{root_id}` does not exist"),
            ));
        };
        self.tree_inner(root, &mut HashSet::new())
    }

    fn tree_inner(
        &self,
        record: &DerivedRecord,
        visiting: &mut HashSet<ChronicleId>,
    ) -> Result<RecordTreeNode, SillokError> {
        if !visiting.insert(record.record_id) {
            return Err(SillokError::new(
                "parent_cycle",
                format!("cycle detected at `{}`", record.record_id),
            ));
        }
        let mut children = Vec::new();
        if let Some(child_ids) = self.children.get(&record.record_id) {
            let mut ordered = child_ids.clone();
            ordered.sort_by_key(|id| {
                self.records
                    .get(id)
                    .map(|child| (child.created_at, child.record_id))
            });
            for child_id in ordered {
                let Some(child) = self.records.get(&child_id) else {
                    continue;
                };
                if child.status == RecordStatus::Retracted {
                    continue;
                }
                children.push(self.tree_inner(child, visiting)?);
            }
        }
        visiting.remove(&record.record_id);
        Ok(RecordTreeNode {
            record: record.clone(),
            children,
        })
    }

    fn push_matching_records(
        &self,
        ids: &[ChronicleId],
        records: &mut Vec<DerivedRecord>,
        filter: RecordQuery<'_>,
    ) {
        for id in ids {
            let Some(record) = self.records.get(id) else {
                continue;
            };
            if record_matches_query(record, filter) {
                records.push(record.clone());
            }
        }
    }
}

fn sort_records(records: &mut [DerivedRecord]) {
    records.sort_by_key(|record| (record.created_at, record.record_id));
}

fn record_matches_query(record: &DerivedRecord, filter: RecordQuery<'_>) -> bool {
    if record.created_at < filter.from || record.created_at > filter.to {
        return false;
    }
    if record.status == RecordStatus::Retracted {
        return false;
    }
    if let Some(required_status) = filter.status
        && record.status != required_status
    {
        return false;
    }
    if let Some(required_tag) = filter.tag
        && !record.tags.iter().any(|value| value == required_tag)
    {
        return false;
    }
    if let Some(required_context) = filter.context
        && !record.context.key_contains(required_context)
    {
        return false;
    }
    true
}
