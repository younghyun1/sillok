//! Frozen copies of the 0.9/0.10 persisted types.
//!
//! bitcode is not self-describing: it encodes fields by position and enum
//! variants by index. These definitions must match 0.10 exactly (field order,
//! variant order, and types) or old archives stop decoding. Never edit them;
//! the golden fixtures in tests/fixtures/v0_10 fail if they drift.

use bitcode::{Decode, Encode};

/// 0.10 `ChronicleId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
pub struct LegacyId(pub [u8; 16]);

/// 0.10 `Timestamp` (UTC milliseconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
pub struct LegacyTimestamp(pub i64);

/// 0.10 `WorkContext`.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LegacyWorkContext {
    pub cwd: Option<String>,
    pub git_root: Option<String>,
    pub git_branch: Option<String>,
    pub git_head: Option<String>,
    pub git_remote: Option<String>,
}

/// 0.10 `RecordStatus`, variant order preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum LegacyStatus {
    Open,
    Active,
    Blocked,
    Completed,
    Retracted,
}

/// 0.10 `DayKey`.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LegacyDayKey {
    pub date: String,
    pub timezone: String,
}

/// 0.10 `EventKind`, variant and field order preserved.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum LegacyEventKind {
    ArchiveInitialized {
        archive_id: LegacyId,
    },
    DayOpened {
        day_id: LegacyId,
        day_key: LegacyDayKey,
    },
    ObjectiveAdded {
        objective_id: LegacyId,
        day_id: LegacyId,
        text: String,
        tags: Vec<String>,
    },
    ObjectiveCompleted {
        objective_id: LegacyId,
        note: Option<String>,
    },
    TaskRecorded {
        task_id: LegacyId,
        day_id: LegacyId,
        parent_id: LegacyId,
        text: String,
        purpose: Option<String>,
        tags: Vec<String>,
        status: LegacyStatus,
    },
    TaskAmended {
        record_id: LegacyId,
        text: Option<String>,
        status: Option<LegacyStatus>,
        purpose: Option<String>,
        tags: Option<Vec<String>>,
    },
    TaskRetracted {
        record_id: LegacyId,
        reason: String,
    },
    TaskLinked {
        child_id: LegacyId,
        parent_id: LegacyId,
    },
    TaskUnlinked {
        child_id: LegacyId,
    },
}

/// 0.10 `ChronicleEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LegacyEvent {
    pub event_id: LegacyId,
    pub event_at: LegacyTimestamp,
    pub recorded_at: LegacyTimestamp,
    pub actor: String,
    pub context: LegacyWorkContext,
    pub kind: LegacyEventKind,
}

/// 0.10 `Archive`: the v1 store file and the v2 sync artifact.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LegacyArchive {
    pub schema_version: u32,
    pub archive_id: LegacyId,
    pub created_at: LegacyTimestamp,
    pub events: Vec<LegacyEvent>,
}
