//! Keeps derived records inside the store's constraints.
//!
//! Local writes are validated before they become events, but synced or
//! imported events come from other machines, other versions, or hand edits.
//! Rejecting one would stall every later sync on every replica, because the
//! event lives on the remote. Instead the projection clamps values to the
//! bounds the schema enforces; the event bytes themselves stay untouched.

use std::collections::BTreeSet;

use crate::domain::record::{Record, RecordStatus};
use crate::domain::text::{DETAIL_MAX_CHARS, ENTRY_MAX_CHARS, TAG_MAX_CHARS, TAGS_MAX};

/// Placeholder for records whose text arrived empty.
const EMPTY_TEXT: &str = "(empty)";
/// Reason for retractions that arrived without one.
const MISSING_REASON: &str = "retracted";

/// Clamps a record to the schema's bounds and invariants.
pub fn sanitize(record: &mut Record) {
    record.text = bounded(&record.text, ENTRY_MAX_CHARS);
    if record.text.is_empty() {
        record.text = EMPTY_TEXT.to_string();
    }
    record.purpose = optional(record.purpose.take());
    record.note = optional(record.note.take());
    let tags: BTreeSet<String> = record
        .tags
        .iter()
        .map(|tag| bounded(tag, TAG_MAX_CHARS))
        .filter(|tag| !tag.is_empty())
        .collect();
    record.tags = tags.into_iter().take(TAGS_MAX).collect();
    if record.prior_status == Some(RecordStatus::Retracted) {
        record.prior_status = Some(RecordStatus::Open);
    }
    match record.status {
        RecordStatus::Retracted => {
            record.retraction_reason = match optional(record.retraction_reason.take()) {
                Some(reason) => Some(reason),
                None => Some(MISSING_REASON.to_string()),
            };
        }
        _ => record.retraction_reason = None,
    }
}

fn optional(value: Option<String>) -> Option<String> {
    match value {
        Some(text) => {
            let clean = bounded(&text, DETAIL_MAX_CHARS);
            if clean.is_empty() { None } else { Some(clean) }
        }
        None => None,
    }
}

/// Trims and truncates on character boundaries.
fn bounded(value: &str, max: usize) -> String {
    value.trim().chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize;
    use crate::domain::event::context::WorkContext;
    use crate::domain::id::RecordId;
    use crate::domain::record::{Record, RecordKind, RecordStatus};
    use crate::domain::time::Timestamp;

    #[test]
    fn foreign_values_are_clamped() {
        let mut record = Record {
            id: RecordId::new_v7(),
            kind: RecordKind::Task,
            parent: None,
            status: RecordStatus::Retracted,
            text: "가".repeat(5000),
            purpose: Some("  ".into()),
            note: None,
            tags: vec!["x".into(), "x".into(), String::new()],
            retraction_reason: Some(String::new()),
            prior_status: Some(RecordStatus::Retracted),
            created_at: Timestamp::from_millis(0),
            updated_at: Timestamp::from_millis(0),
            context: WorkContext::default(),
        };
        sanitize(&mut record);
        assert_eq!(record.text.chars().count(), 4096);
        assert_eq!(record.purpose, None);
        assert_eq!(record.tags, vec!["x".to_string()]);
        assert_eq!(record.retraction_reason.as_deref(), Some("retracted"));
        assert_eq!(record.prior_status, Some(RecordStatus::Open));
    }
}
