//! Per-event state transitions shared by full replay and the live write path.
//!
//! Keeping one implementation guarantees that a record updated incrementally
//! by a CLI write equals the record `doctor` derives by replaying every event.

use crate::domain::event::envelope::EventBody;
use crate::domain::event::kind::EventKind;
use crate::domain::record::{Record, RecordKind, RecordStatus};
use crate::domain::reducer::sanitize::sanitize;

/// Reason attached when data creates or amends a record straight to `retracted`.
const CREATED_RETRACTED: &str = "created as retracted";
const AMENDED_RETRACTED: &str = "retracted through amend";

/// Builds the initial record for a creation event.
pub fn create(event: &EventBody) -> Option<Record> {
    let (kind, record_id, parent_id, text, purpose, tags, status) = match &event.kind {
        EventKind::ObjectiveAdded {
            record_id,
            parent_id,
            text,
            tags,
            status,
        } => (
            RecordKind::Objective,
            *record_id,
            *parent_id,
            text,
            None,
            tags,
            *status,
        ),
        EventKind::TaskRecorded {
            record_id,
            parent_id,
            text,
            purpose,
            tags,
            status,
        } => (
            RecordKind::Task,
            *record_id,
            *parent_id,
            text,
            purpose.clone(),
            tags,
            *status,
        ),
        _ => return None,
    };
    // A creation event cannot carry a retraction reason, so a record born
    // retracted (only possible in hand-written or foreign data) gets one here
    // to keep the retracted-implies-reason invariant the schema enforces.
    let (status, prior_status, retraction_reason) = match status {
        RecordStatus::Retracted => (
            RecordStatus::Retracted,
            Some(RecordStatus::Open),
            Some(CREATED_RETRACTED.to_string()),
        ),
        other => (other, None, None),
    };
    let mut record = Record {
        id: record_id,
        kind,
        parent: parent_id,
        status,
        text: text.clone(),
        purpose,
        note: None,
        tags: tags.clone(),
        retraction_reason,
        prior_status,
        created_at: event.event_at,
        updated_at: event.recorded_at,
        context: event.context.clone(),
    };
    sanitize(&mut record);
    Some(record)
}

/// Applies an amend, retract, or restore. Returns false when the event had
/// no effect (for example restoring a record that is not retracted).
///
/// Moves are not handled here because they need the whole parent graph to
/// reject cycles; see `replay` and the SQLite write path.
pub fn apply(record: &mut Record, event: &EventBody) -> bool {
    let changed = match &event.kind {
        EventKind::RecordAmended {
            text,
            status,
            purpose,
            clear_purpose,
            tags,
            note,
            ..
        } => {
            if let Some(value) = text {
                record.text = value.clone();
            }
            if let Some(value) = status {
                set_status(record, *value);
            }
            if *clear_purpose {
                record.purpose = None;
            }
            if let Some(value) = purpose {
                record.purpose = Some(value.clone());
            }
            if let Some(value) = tags {
                record.tags = value.clone();
            }
            if let Some(value) = note {
                record.note = Some(value.clone());
            }
            true
        }
        EventKind::RecordRetracted { reason, .. } if record.status != RecordStatus::Retracted => {
            record.prior_status = Some(record.status);
            record.status = RecordStatus::Retracted;
            record.retraction_reason = Some(reason.clone());
            true
        }
        EventKind::RecordRestored { .. } if record.status == RecordStatus::Retracted => {
            record.status = match record.prior_status {
                Some(previous) => previous,
                None => RecordStatus::Open,
            };
            record.prior_status = None;
            record.retraction_reason = None;
            true
        }
        _ => false,
    };
    if changed {
        sanitize(record);
        touch(record, event);
    }
    changed
}

/// Advances `updated_at` monotonically; replay order can apply an older
/// clock-skewed event after a newer one.
pub fn touch(record: &mut Record, event: &EventBody) {
    if event.recorded_at > record.updated_at {
        record.updated_at = event.recorded_at;
    }
}

/// While retracted, a status change is remembered for `restore` instead of
/// un-hiding the record.
fn set_status(record: &mut Record, status: RecordStatus) {
    if record.status == RecordStatus::Retracted && status != RecordStatus::Retracted {
        record.prior_status = Some(status);
    } else if status == RecordStatus::Retracted && record.status != RecordStatus::Retracted {
        record.prior_status = Some(record.status);
        record.status = status;
        if record.retraction_reason.is_none() {
            record.retraction_reason = Some(AMENDED_RETRACTED.to_string());
        }
    } else {
        record.status = status;
    }
}
