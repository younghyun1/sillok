use std::collections::HashSet;

use crate::domain::event::{ChronicleEvent, EventKind, RecordKind, RecordStatus};
use crate::domain::id::ChronicleId;
use crate::domain::view::{ChronicleView, DerivedRecord};
use crate::error::SillokError;

/// Applies one event to the derived view.
pub fn apply_event(
    view: &mut ChronicleView<'_>,
    event: &ChronicleEvent,
) -> Result<(), SillokError> {
    match &event.kind {
        EventKind::ArchiveInitialized { .. } => Ok(()),
        EventKind::DayOpened { day_id, day_key } => {
            let record = DerivedRecord {
                record_id: *day_id,
                kind: RecordKind::Day,
                day_id: *day_id,
                parent_id: None,
                text: format!("Day {}", day_key.date),
                purpose: None,
                tags: Vec::new(),
                status: RecordStatus::Open,
                created_at: event.event_at,
                updated_at: event.recorded_at,
                context: event.context.clone(),
                retraction_reason: None,
                day_key: Some(day_key.clone()),
            };
            view.records.insert(*day_id, record);
            view.day_by_key.insert(day_key.clone(), *day_id);
            Ok(())
        }
        EventKind::ObjectiveAdded {
            objective_id,
            day_id,
            text,
            tags,
        } => {
            require_record(view, day_id)?;
            let record = DerivedRecord {
                record_id: *objective_id,
                kind: RecordKind::Objective,
                day_id: *day_id,
                parent_id: Some(*day_id),
                text: text.clone(),
                purpose: None,
                tags: tags.clone(),
                status: RecordStatus::Open,
                created_at: event.event_at,
                updated_at: event.recorded_at,
                context: event.context.clone(),
                retraction_reason: None,
                day_key: None,
            };
            view.records.insert(*objective_id, record);
            set_parent(view, *objective_id, *day_id);
            Ok(())
        }
        EventKind::ObjectiveCompleted { objective_id, note } => {
            let record = require_record_mut(view, objective_id)?;
            record.status = RecordStatus::Completed;
            record.updated_at = event.recorded_at;
            if let Some(value) = note {
                record.purpose = Some(value.clone());
            }
            Ok(())
        }
        EventKind::TaskRecorded {
            task_id,
            day_id,
            parent_id,
            text,
            purpose,
            tags,
            status,
        } => {
            require_record(view, day_id)?;
            require_record(view, parent_id)?;
            let record = DerivedRecord {
                record_id: *task_id,
                kind: RecordKind::Task,
                day_id: *day_id,
                parent_id: Some(*parent_id),
                text: text.clone(),
                purpose: purpose.clone(),
                tags: tags.clone(),
                status: *status,
                created_at: event.event_at,
                updated_at: event.recorded_at,
                context: event.context.clone(),
                retraction_reason: None,
                day_key: None,
            };
            view.records.insert(*task_id, record);
            set_parent(view, *task_id, *parent_id);
            Ok(())
        }
        EventKind::TaskAmended {
            record_id,
            text,
            status,
            purpose,
            tags,
        } => {
            let record = require_record_mut(view, record_id)?;
            if let Some(value) = text {
                record.text = value.clone();
            }
            if let Some(value) = status {
                record.status = *value;
            }
            if let Some(value) = purpose {
                record.purpose = Some(value.clone());
            }
            if let Some(value) = tags {
                record.tags = value.clone();
            }
            record.updated_at = event.recorded_at;
            Ok(())
        }
        EventKind::TaskRetracted { record_id, reason } => {
            let record = require_record_mut(view, record_id)?;
            record.status = RecordStatus::Retracted;
            record.retraction_reason = Some(reason.clone());
            record.updated_at = event.recorded_at;
            Ok(())
        }
        EventKind::TaskLinked {
            child_id,
            parent_id,
        } => {
            require_record(view, parent_id)?;
            {
                let record = require_record_mut(view, child_id)?;
                record.parent_id = Some(*parent_id);
                record.updated_at = event.recorded_at;
            }
            set_parent(view, *child_id, *parent_id);
            Ok(())
        }
        EventKind::TaskUnlinked { child_id } => {
            {
                let record = require_record_mut(view, child_id)?;
                record.parent_id = None;
                record.updated_at = event.recorded_at;
            }
            view.parent_by_child.remove(child_id);
            remove_child(view, *child_id);
            Ok(())
        }
    }
}

/// Rebuilds secondary indexes after all events are reduced.
pub fn rebuild_indexes(view: &mut ChronicleView<'_>) {
    view.timeline.clear();
    view.by_tag.clear();
    view.by_context.clear();
    view.by_status.clear();

    for record in view.records.values() {
        view.timeline
            .entry(record.created_at)
            .or_default()
            .push(record.record_id);
        for tag in &record.tags {
            view.by_tag
                .entry(tag.clone())
                .or_default()
                .push(record.record_id);
        }
        view.by_context
            .entry(record.context.key())
            .or_default()
            .push(record.record_id);
        view.by_status
            .entry(record.status)
            .or_default()
            .push(record.record_id);
    }
}

/// Validates that parent pointers do not form cycles.
pub fn validate_parent_graph(view: &ChronicleView<'_>) -> Result<(), SillokError> {
    for record_id in view.records.keys() {
        let mut seen = HashSet::new();
        let mut current = *record_id;
        while let Some(parent) = view.parent_by_child.get(&current) {
            if !seen.insert(current) {
                return Err(SillokError::new(
                    "parent_cycle",
                    format!("parent cycle includes `{current}`"),
                ));
            }
            current = *parent;
        }
    }
    Ok(())
}

fn require_record(view: &ChronicleView<'_>, id: &ChronicleId) -> Result<(), SillokError> {
    if view.records.contains_key(id) {
        Ok(())
    } else {
        Err(SillokError::new(
            "record_not_found",
            format!("record `{id}` does not exist"),
        ))
    }
}

fn require_record_mut<'a>(
    view: &'a mut ChronicleView<'_>,
    id: &ChronicleId,
) -> Result<&'a mut DerivedRecord, SillokError> {
    match view.records.get_mut(id) {
        Some(record) => Ok(record),
        None => Err(SillokError::new(
            "record_not_found",
            format!("record `{id}` does not exist"),
        )),
    }
}

fn set_parent(view: &mut ChronicleView<'_>, child_id: ChronicleId, parent_id: ChronicleId) {
    remove_child(view, child_id);
    view.parent_by_child.insert(child_id, parent_id);
    view.children.entry(parent_id).or_default().push(child_id);
}

fn remove_child(view: &mut ChronicleView<'_>, child_id: ChronicleId) {
    let mut empty_parents = Vec::new();
    for (parent_id, children) in &mut view.children {
        children.retain(|value| value != &child_id);
        if children.is_empty() {
            empty_parents.push(*parent_id);
        }
    }
    for parent_id in empty_parents {
        view.children.remove(&parent_id);
    }
}
