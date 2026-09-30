//! Record kinds, statuses, and the derived record state.

use serde::{Deserialize, Serialize};

use crate::domain::event::context::WorkContext;
use crate::domain::id::RecordId;
use crate::domain::time::Timestamp;
use crate::error::SillokError;

/// What a record represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    /// A goal that groups work and may span many days.
    Objective,
    /// One unit of work or a note about it.
    Task,
}

impl RecordKind {
    /// Stable storage and output label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Objective => "objective",
            Self::Task => "task",
        }
    }

    /// Parses a storage label.
    pub fn parse(value: &str) -> Result<Self, SillokError> {
        match value {
            "objective" => Ok(Self::Objective),
            "task" => Ok(Self::Task),
            other => Err(SillokError::datashape(
                "invalid_datashape",
                format!("unknown record kind `{other}`"),
            )),
        }
    }
}

/// Lifecycle state of a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordStatus {
    Open,
    Active,
    Blocked,
    Completed,
    /// Hidden from normal views; reachable only through `retract`.
    Retracted,
}

impl RecordStatus {
    /// Stable storage and output label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Active => "active",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Retracted => "retracted",
        }
    }

    /// Parses a storage label.
    pub fn parse(value: &str) -> Result<Self, SillokError> {
        match value {
            "open" => Ok(Self::Open),
            "active" => Ok(Self::Active),
            "blocked" => Ok(Self::Blocked),
            "completed" => Ok(Self::Completed),
            "retracted" => Ok(Self::Retracted),
            other => Err(SillokError::datashape(
                "invalid_datashape",
                format!("unknown record status `{other}`"),
            )),
        }
    }

    /// Whether work on the record is still in flight.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Open | Self::Active | Self::Blocked)
    }
}

/// Current state of one record, derived from its events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Record {
    pub id: RecordId,
    pub kind: RecordKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<RecordId>,
    pub status: RecordStatus,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retraction_reason: Option<String>,
    /// Status to return to on `restore`; set only while retracted.
    #[serde(skip)]
    pub prior_status: Option<RecordStatus>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(skip)]
    pub context: WorkContext,
}

#[cfg(test)]
mod tests {
    use super::{RecordKind, RecordStatus};

    #[test]
    fn labels_roundtrip() {
        for status in [
            RecordStatus::Open,
            RecordStatus::Active,
            RecordStatus::Blocked,
            RecordStatus::Completed,
            RecordStatus::Retracted,
        ] {
            assert!(matches!(RecordStatus::parse(status.as_str()), Ok(value) if value == status));
        }
        for kind in [RecordKind::Objective, RecordKind::Task] {
            assert!(matches!(RecordKind::parse(kind.as_str()), Ok(value) if value == kind));
        }
        assert!(RecordKind::parse("day").is_err());
    }
}
