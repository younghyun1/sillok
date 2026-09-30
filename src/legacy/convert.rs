//! Deterministic translation of 0.9/0.10 events into v3 events.
//!
//! Two machines that migrate the same 0.10 data independently must produce
//! byte-identical JSON, or sync would see conflicting payloads for one event
//! id. Everything here is a pure function of the legacy event: ids are
//! reused (or derived with `EventId::derived`), timestamps are copied, and
//! serialization goes through the fixed field order of `EventBody`.
//!
//! What changes in meaning (documented as the accepted 1.0 discrepancies):
//! - `ArchiveInitialized` and `DayOpened` are dropped; days are derived now.
//! - A parent that was a Day record becomes `None` (top-level).
//! - `ObjectiveCompleted.note` moves from `purpose` to the new `note` field.
//! - An amend that set `retracted`, or a task created as `retracted`, gains a
//!   `record_retracted` event (derived id) instead.
//! - Events that amended, retracted, or linked Day records are dropped.

use std::collections::HashSet;

use crate::domain::event::context::{WorkContext, sanitize_remote};
use crate::domain::event::envelope::{Event, EventBody};
use crate::domain::event::kind::EventKind;
use crate::domain::id::{ArchiveId, EventId, RecordId};
use crate::domain::record::RecordStatus;
use crate::domain::time::Timestamp;
use crate::error::SillokError;
use crate::legacy::types::{LegacyArchive, LegacyEvent, LegacyEventKind, LegacyId, LegacyStatus};

/// Reason recorded when a 0.10 amend had set a record to `retracted`.
pub const AMEND_RETRACT_REASON: &str = "imported from 0.10: retracted through amend";
/// Reason recorded when a 0.10 task was created with status `retracted`.
pub const CREATED_RETRACTED_REASON: &str = "imported from 0.10: recorded as retracted";

/// Result of converting one legacy archive.
#[derive(Debug)]
pub struct Conversion {
    pub archive_id: ArchiveId,
    pub created_at: Timestamp,
    pub events: Vec<Event>,
    /// Legacy events with no 1.0 equivalent (archive and day markers).
    pub dropped: usize,
}

/// Converts every event of a legacy archive.
pub fn convert(archive: &LegacyArchive) -> Result<Conversion, SillokError> {
    let days: HashSet<LegacyId> = archive
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            LegacyEventKind::DayOpened { day_id, .. } => Some(*day_id),
            _ => None,
        })
        .collect();
    let mut events = Vec::with_capacity(archive.events.len());
    let mut dropped = 0usize;
    for event in &archive.events {
        let kinds = convert_kind(event, &days);
        if kinds.is_empty() {
            dropped += 1;
        }
        for (index, kind) in kinds.into_iter().enumerate() {
            let event_id = match index {
                0 => EventId::from_bytes(event.event_id.0),
                _ => EventId::derived(event.event_id.0, "split"),
            };
            match Event::from_body(body(event, event_id, kind)) {
                Ok(value) => events.push(value),
                Err(error) => return Err(error),
            }
        }
    }
    Ok(Conversion {
        archive_id: ArchiveId::from_bytes(archive.archive_id.0),
        created_at: Timestamp::from_millis(archive.created_at.0),
        events,
        dropped,
    })
}

fn body(event: &LegacyEvent, event_id: EventId, kind: EventKind) -> EventBody {
    EventBody {
        event_id,
        event_at: Timestamp::from_millis(event.event_at.0),
        recorded_at: Timestamp::from_millis(event.recorded_at.0),
        actor: event.actor.clone(),
        context: WorkContext {
            cwd: event.context.cwd.clone(),
            git_root: event.context.git_root.clone(),
            git_branch: event.context.git_branch.clone(),
            git_head: event.context.git_head.clone(),
            git_remote: event.context.git_remote.as_deref().map(sanitize_remote),
            session: None,
        },
        kind,
    }
}

fn record(id: LegacyId) -> RecordId {
    RecordId::from_bytes(id.0)
}

fn parent(id: LegacyId, days: &HashSet<LegacyId>) -> Option<RecordId> {
    if days.contains(&id) {
        None
    } else {
        Some(record(id))
    }
}

fn status(value: LegacyStatus) -> RecordStatus {
    match value {
        LegacyStatus::Open => RecordStatus::Open,
        LegacyStatus::Active => RecordStatus::Active,
        LegacyStatus::Blocked => RecordStatus::Blocked,
        LegacyStatus::Completed => RecordStatus::Completed,
        LegacyStatus::Retracted => RecordStatus::Retracted,
    }
}

fn convert_kind(event: &LegacyEvent, days: &HashSet<LegacyId>) -> Vec<EventKind> {
    match &event.kind {
        LegacyEventKind::ArchiveInitialized { .. } | LegacyEventKind::DayOpened { .. } => {
            Vec::new()
        }
        LegacyEventKind::ObjectiveAdded {
            objective_id,
            text,
            tags,
            ..
        } => vec![EventKind::ObjectiveAdded {
            record_id: record(*objective_id),
            parent_id: None,
            text: text.clone(),
            tags: tags.clone(),
            status: RecordStatus::Open,
        }],
        LegacyEventKind::ObjectiveCompleted { objective_id, note } => {
            vec![EventKind::RecordAmended {
                record_id: record(*objective_id),
                text: None,
                status: Some(RecordStatus::Completed),
                purpose: None,
                clear_purpose: false,
                tags: None,
                note: note.clone(),
            }]
        }
        LegacyEventKind::TaskRecorded {
            task_id,
            parent_id,
            text,
            purpose,
            tags,
            status: task_status,
            ..
        } => {
            let retracted = *task_status == LegacyStatus::Retracted;
            let mut kinds = vec![EventKind::TaskRecorded {
                record_id: record(*task_id),
                parent_id: parent(*parent_id, days),
                text: text.clone(),
                purpose: purpose.clone(),
                tags: tags.clone(),
                status: match retracted {
                    true => RecordStatus::Open,
                    false => status(*task_status),
                },
            }];
            if retracted {
                kinds.push(EventKind::RecordRetracted {
                    record_id: record(*task_id),
                    reason: CREATED_RETRACTED_REASON.to_string(),
                });
            }
            kinds
        }
        LegacyEventKind::TaskAmended {
            record_id,
            text,
            status: amended_status,
            purpose,
            tags,
        } => {
            if days.contains(record_id) {
                return Vec::new();
            }
            let retract = *amended_status == Some(LegacyStatus::Retracted);
            let mut kinds = Vec::with_capacity(2);
            if retract {
                kinds.push(EventKind::RecordRetracted {
                    record_id: record(*record_id),
                    reason: AMEND_RETRACT_REASON.to_string(),
                });
            }
            let status_change = match amended_status {
                Some(LegacyStatus::Retracted) | None => None,
                Some(value) => Some(status(*value)),
            };
            if text.is_some() || status_change.is_some() || purpose.is_some() || tags.is_some() {
                kinds.push(EventKind::RecordAmended {
                    record_id: record(*record_id),
                    text: text.clone(),
                    status: status_change,
                    purpose: purpose.clone(),
                    clear_purpose: false,
                    tags: tags.clone(),
                    note: None,
                });
            }
            kinds
        }
        LegacyEventKind::TaskRetracted { record_id, reason } => {
            if days.contains(record_id) {
                return Vec::new();
            }
            vec![EventKind::RecordRetracted {
                record_id: record(*record_id),
                reason: reason.clone(),
            }]
        }
        LegacyEventKind::TaskLinked {
            child_id,
            parent_id,
        } => {
            if days.contains(child_id) {
                return Vec::new();
            }
            vec![EventKind::RecordMoved {
                record_id: record(*child_id),
                parent_id: parent(*parent_id, days),
            }]
        }
        LegacyEventKind::TaskUnlinked { child_id } => {
            if days.contains(child_id) {
                return Vec::new();
            }
            vec![EventKind::RecordMoved {
                record_id: record(*child_id),
                parent_id: None,
            }]
        }
    }
}
