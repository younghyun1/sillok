//! The v3 event vocabulary.
//!
//! Evolution rules (see docs/architecture/be/event-format.md): new fields must
//! be optional with serde defaults, new variants get new `type` names, and no
//! existing field or variant is renamed or repurposed. Unknown types decode to
//! `Unknown` so older readers keep and sync events they cannot interpret.

use serde::{Deserialize, Serialize};

use crate::domain::id::RecordId;
use crate::domain::record::{RecordKind, RecordStatus};

fn is_false(value: &bool) -> bool {
    !*value
}

/// One state change in the chronicle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    /// Creates an objective.
    ObjectiveAdded {
        record_id: RecordId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_id: Option<RecordId>,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
        status: RecordStatus,
    },
    /// Creates a task.
    TaskRecorded {
        record_id: RecordId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_id: Option<RecordId>,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        purpose: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
        status: RecordStatus,
    },
    /// Changes supplied fields; absent fields are untouched.
    RecordAmended {
        record_id: RecordId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<RecordStatus>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        purpose: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        clear_purpose: bool,
        /// `Some([])` clears all tags.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tags: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Re-parents a record; `None` makes it top-level.
    RecordMoved {
        record_id: RecordId,
        parent_id: Option<RecordId>,
    },
    /// Hides a record from normal views.
    RecordRetracted { record_id: RecordId, reason: String },
    /// Reverses a retraction.
    RecordRestored { record_id: RecordId },
    /// A type written by a newer Sillok; kept and synced, never applied.
    #[serde(other)]
    Unknown,
}

impl EventKind {
    /// The record this event is about.
    pub fn record_id(&self) -> Option<RecordId> {
        match self {
            Self::ObjectiveAdded { record_id, .. }
            | Self::TaskRecorded { record_id, .. }
            | Self::RecordAmended { record_id, .. }
            | Self::RecordMoved { record_id, .. }
            | Self::RecordRetracted { record_id, .. }
            | Self::RecordRestored { record_id } => Some(*record_id),
            Self::Unknown => None,
        }
    }

    /// Stable `type` label, also stored in the event table.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ObjectiveAdded { .. } => "objective_added",
            Self::TaskRecorded { .. } => "task_recorded",
            Self::RecordAmended { .. } => "record_amended",
            Self::RecordMoved { .. } => "record_moved",
            Self::RecordRetracted { .. } => "record_retracted",
            Self::RecordRestored { .. } => "record_restored",
            Self::Unknown => "unknown",
        }
    }

    /// The kind of record this event creates, if it is a creation event.
    pub fn created_kind(&self) -> Option<RecordKind> {
        match self {
            Self::ObjectiveAdded { .. } => Some(RecordKind::Objective),
            Self::TaskRecorded { .. } => Some(RecordKind::Task),
            _ => None,
        }
    }

    /// What happened, in the words day views use.
    pub fn activity(&self) -> &'static str {
        match self {
            Self::ObjectiveAdded { .. } | Self::TaskRecorded { .. } => "recorded",
            Self::RecordAmended {
                status: Some(RecordStatus::Completed),
                ..
            } => "completed",
            Self::RecordAmended { .. } => "amended",
            Self::RecordMoved { .. } => "moved",
            Self::RecordRetracted { .. } => "retracted",
            Self::RecordRestored { .. } => "restored",
            Self::Unknown => "unknown",
        }
    }
}
