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
    pub timeline: BTreeMap<Timestamp, Vec<ChronicleId>>,
    pub by_tag: HashMap<String, Vec<ChronicleId>>,
    pub by_context: HashMap<String, Vec<ChronicleId>>,
    pub by_status: HashMap<RecordStatus, Vec<ChronicleId>>,
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

        let mut view = Self {
            archive,
            records: HashMap::new(),
            children: HashMap::new(),
            parent_by_child: HashMap::new(),
            day_by_key: HashMap::new(),
            timeline: BTreeMap::new(),
            by_tag: HashMap::new(),
            by_context: HashMap::new(),
            by_status: HashMap::new(),
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
        let mut records = Vec::new();
        for record in self.records.values() {
            if record.status != RecordStatus::Retracted {
                records.push(record.clone());
            }
        }
        sort_records(&mut records);
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
        let mut ids = HashSet::new();
        for (_timestamp, bucket) in self.timeline.range(from..=to) {
            for id in bucket {
                ids.insert(*id);
            }
        }

        let mut records = Vec::new();
        for id in ids {
            let Some(record) = self.records.get(&id) else {
                continue;
            };
            if record.status == RecordStatus::Retracted {
                continue;
            }
            if let Some(required_status) = status
                && record.status != required_status
            {
                continue;
            }
            if let Some(required_tag) = tag
                && !record.tags.iter().any(|value| value == required_tag)
            {
                continue;
            }
            if let Some(required_context) = context {
                let key = record.context.key();
                if !key.contains(required_context) {
                    continue;
                }
            }
            records.push(record.clone());
        }
        sort_records(&mut records);
        records
    }

    /// Returns the event history affecting one record.
    pub fn events_for_record(&self, id: ChronicleId) -> Vec<ChronicleEvent> {
        let mut events = Vec::new();
        for event in self.archive.events.iter() {
            if event.kind.referenced_ids().iter().any(|value| value == &id) {
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
}

fn sort_records(records: &mut [DerivedRecord]) {
    records.sort_by_key(|record| (record.created_at, record.record_id));
}
