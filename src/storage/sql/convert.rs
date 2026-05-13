use crate::domain::event::{EventKind, RecordKind, RecordStatus, WorkContext};
use crate::domain::id::ChronicleId;
use crate::domain::time::{DayKey, Timestamp};
use crate::error::SillokError;

/// Converts a chronicle id into compact SQL BLOB storage.
pub fn id_blob(id: ChronicleId) -> Vec<u8> {
    id.to_vec()
}

/// Converts an optional id into compact SQL BLOB storage.
pub fn optional_id_blob(id: Option<ChronicleId>) -> Option<Vec<u8>> {
    id.map(|value| value.to_vec())
}

/// Converts a record kind into its SQL label.
pub fn kind_label(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Day => "day",
        RecordKind::Task => "task",
        RecordKind::Objective => "objective",
    }
}

/// Parses a record kind SQL label.
pub fn parse_kind(value: &str) -> Result<RecordKind, SillokError> {
    match value {
        "day" => Ok(RecordKind::Day),
        "task" => Ok(RecordKind::Task),
        "objective" => Ok(RecordKind::Objective),
        _ => Err(SillokError::new(
            "invalid_datashape",
            format!("unknown record kind `{value}`"),
        )),
    }
}

/// Converts a record status into its SQL label.
pub fn status_label(status: RecordStatus) -> &'static str {
    match status {
        RecordStatus::Open => "open",
        RecordStatus::Active => "active",
        RecordStatus::Blocked => "blocked",
        RecordStatus::Completed => "completed",
        RecordStatus::Retracted => "retracted",
    }
}

/// Parses a record status SQL label.
pub fn parse_status(value: &str) -> Result<RecordStatus, SillokError> {
    match value {
        "open" => Ok(RecordStatus::Open),
        "active" => Ok(RecordStatus::Active),
        "blocked" => Ok(RecordStatus::Blocked),
        "completed" => Ok(RecordStatus::Completed),
        "retracted" => Ok(RecordStatus::Retracted),
        _ => Err(SillokError::new(
            "invalid_datashape",
            format!("unknown record status `{value}`"),
        )),
    }
}

/// Returns a compact event-kind discriminator for indexes.
pub fn event_kind_label(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::ArchiveInitialized { .. } => "archive_initialized",
        EventKind::DayOpened { .. } => "day_opened",
        EventKind::ObjectiveAdded { .. } => "objective_added",
        EventKind::ObjectiveCompleted { .. } => "objective_completed",
        EventKind::TaskRecorded { .. } => "task_recorded",
        EventKind::TaskAmended { .. } => "task_amended",
        EventKind::TaskRetracted { .. } => "task_retracted",
        EventKind::TaskLinked { .. } => "task_linked",
        EventKind::TaskUnlinked { .. } => "task_unlinked",
    }
}

/// Builds a persisted day key from SQL columns.
pub fn day_key(date: String, timezone: String) -> DayKey {
    DayKey { date, timezone }
}

/// Builds a timestamp from an integer SQL column.
pub fn timestamp(millis: i64) -> Timestamp {
    Timestamp::from_millis(millis)
}

/// Serializes a work context into a stable JSON key for deduplication.
pub fn context_json(context: &WorkContext) -> Result<String, SillokError> {
    Ok(serde_json::to_string(context)?)
}
