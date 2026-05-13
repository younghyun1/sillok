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
    /// Returns a compact context key for indexing.
    pub fn key(&self) -> String {
        match &self.git_root {
            Some(value) => value.clone(),
            None => match &self.cwd {
                Some(value) => value.clone(),
                None => "unknown".to_string(),
            },
        }
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
