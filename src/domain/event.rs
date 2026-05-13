use bitcode::{Decode, Encode};
use clap::ValueEnum;
use serde::Serialize;

use crate::domain::id::ChronicleId;
use crate::domain::time::{DayKey, Timestamp};

/// Runtime context captured at event creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Encode, Decode)]
pub struct WorkContext {
    pub cwd: Option<String>,
    pub git_root: Option<String>,
    pub git_branch: Option<String>,
    pub git_head: Option<String>,
    pub git_remote: Option<String>,
}

impl WorkContext {
    /// Returns the context key without allocating when possible.
    pub fn key_ref(&self) -> &str {
        match &self.git_root {
            Some(value) => value.as_str(),
            None => match &self.cwd {
                Some(value) => value.as_str(),
                None => "unknown",
            },
        }
    }

    /// Returns a compact context key for indexing.
    pub fn key(&self) -> String {
        self.key_ref().to_string()
    }

    /// Returns whether the compact context key contains a fragment.
    pub fn key_contains(&self, fragment: &str) -> bool {
        self.key_ref().contains(fragment)
    }
}

/// Current lifecycle state derived for a chronicle record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, ValueEnum, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum RecordStatus {
    Open,
    Active,
    Blocked,
    Completed,
    Retracted,
}

impl RecordStatus {
    /// Returns the number of status variants for small fixed-capacity indexes.
    pub const fn variant_count() -> usize {
        5
    }
}

/// Record kind in the derived chronicle view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Day,
    Task,
    Objective,
}

/// One append-only event in the archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Encode, Decode)]
pub struct ChronicleEvent {
    pub event_id: ChronicleId,
    pub event_at: Timestamp,
    pub recorded_at: Timestamp,
    pub actor: String,
    pub context: WorkContext,
    pub kind: EventKind,
}

impl ChronicleEvent {
    /// Creates an event with a fresh event id.
    pub fn new(
        event_at: Timestamp,
        recorded_at: Timestamp,
        actor: String,
        context: WorkContext,
        kind: EventKind,
    ) -> Self {
        Self {
            event_id: ChronicleId::new_v7(),
            event_at,
            recorded_at,
            actor,
            context,
            kind,
        }
    }

    /// Returns the primary record id affected by this event.
    pub fn primary_record_id(&self) -> Option<ChronicleId> {
        self.kind.primary_record_id()
    }

    /// Returns whether this event references a record id.
    pub fn references(&self, id: ChronicleId) -> bool {
        self.kind.references(id)
    }
}

/// Domain mutation recorded in the archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Encode, Decode)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    ArchiveInitialized {
        archive_id: ChronicleId,
    },
    DayOpened {
        day_id: ChronicleId,
        day_key: DayKey,
    },
    ObjectiveAdded {
        objective_id: ChronicleId,
        day_id: ChronicleId,
        text: String,
        tags: Vec<String>,
    },
    ObjectiveCompleted {
        objective_id: ChronicleId,
        note: Option<String>,
    },
    TaskRecorded {
        task_id: ChronicleId,
        day_id: ChronicleId,
        parent_id: ChronicleId,
        text: String,
        purpose: Option<String>,
        tags: Vec<String>,
        status: RecordStatus,
    },
    TaskAmended {
        record_id: ChronicleId,
        text: Option<String>,
        status: Option<RecordStatus>,
        purpose: Option<String>,
        tags: Option<Vec<String>>,
    },
    TaskRetracted {
        record_id: ChronicleId,
        reason: String,
    },
    TaskLinked {
        child_id: ChronicleId,
        parent_id: ChronicleId,
    },
    TaskUnlinked {
        child_id: ChronicleId,
    },
}

impl EventKind {
    /// Returns the primary record id affected by this event.
    pub fn primary_record_id(&self) -> Option<ChronicleId> {
        match self {
            Self::ArchiveInitialized { .. } => None,
            Self::DayOpened { day_id, .. } => Some(*day_id),
            Self::ObjectiveAdded { objective_id, .. } => Some(*objective_id),
            Self::ObjectiveCompleted { objective_id, .. } => Some(*objective_id),
            Self::TaskRecorded { task_id, .. } => Some(*task_id),
            Self::TaskAmended { record_id, .. } => Some(*record_id),
            Self::TaskRetracted { record_id, .. } => Some(*record_id),
            Self::TaskLinked { child_id, .. } => Some(*child_id),
            Self::TaskUnlinked { child_id } => Some(*child_id),
        }
    }

    /// Returns whether this event references the supplied record id.
    pub fn references(&self, id: ChronicleId) -> bool {
        match self {
            Self::ArchiveInitialized { archive_id } => *archive_id == id,
            Self::DayOpened { day_id, .. } => *day_id == id,
            Self::ObjectiveAdded {
                objective_id,
                day_id,
                ..
            } => *objective_id == id || *day_id == id,
            Self::ObjectiveCompleted { objective_id, .. } => *objective_id == id,
            Self::TaskRecorded {
                task_id,
                day_id,
                parent_id,
                ..
            } => *task_id == id || *day_id == id || *parent_id == id,
            Self::TaskAmended { record_id, .. } => *record_id == id,
            Self::TaskRetracted { record_id, .. } => *record_id == id,
            Self::TaskLinked {
                child_id,
                parent_id,
            } => *child_id == id || *parent_id == id,
            Self::TaskUnlinked { child_id } => *child_id == id,
        }
    }

    /// Returns all record ids referenced by this event.
    pub fn referenced_ids(&self) -> Vec<ChronicleId> {
        match self {
            Self::ArchiveInitialized { archive_id } => vec![*archive_id],
            Self::DayOpened { day_id, .. } => vec![*day_id],
            Self::ObjectiveAdded {
                objective_id,
                day_id,
                ..
            } => vec![*objective_id, *day_id],
            Self::ObjectiveCompleted { objective_id, .. } => vec![*objective_id],
            Self::TaskRecorded {
                task_id,
                day_id,
                parent_id,
                ..
            } => vec![*task_id, *day_id, *parent_id],
            Self::TaskAmended { record_id, .. } => vec![*record_id],
            Self::TaskRetracted { record_id, .. } => vec![*record_id],
            Self::TaskLinked {
                child_id,
                parent_id,
            } => vec![*child_id, *parent_id],
            Self::TaskUnlinked { child_id } => vec![*child_id],
        }
    }
}
