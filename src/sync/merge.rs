use std::collections::{BTreeSet, HashMap, HashSet};

use crate::domain::archive::{ARCHIVE_SCHEMA_VERSION, Archive};
use crate::domain::event::{ChronicleEvent, EventKind};
use crate::domain::id::ChronicleId;
use crate::domain::time::Timestamp;
use crate::domain::view::ChronicleView;
use crate::error::SillokError;

/// Result of merging local and remote archives.
#[derive(Debug, Clone)]
pub struct MergeOutcome {
    pub archive: Option<Archive>,
    pub merged: bool,
}

/// Merges two optional archives by event id and validates the resulting view.
pub fn merge_archives(
    local: Option<&Archive>,
    remote: Option<&Archive>,
) -> Result<MergeOutcome, SillokError> {
    match (local, remote) {
        (None, None) => Ok(MergeOutcome {
            archive: None,
            merged: false,
        }),
        (Some(archive), None) | (None, Some(archive)) => {
            ChronicleView::build(archive)?;
            Ok(MergeOutcome {
                archive: Some(archive.clone()),
                merged: false,
            })
        }
        (Some(local_archive), Some(remote_archive)) => {
            // Archives with different ids mesh too: events union by event_id
            // and the older identity survives. The survivor must be a pure
            // function of the two inputs so every replica converges on a
            // byte-identical artifact instead of ping-ponging ids through the
            // remote; (created_at, archive_id) is a deterministic total order.
            let (archive_id, created_at) = if (local_archive.created_at, local_archive.archive_id)
                <= (remote_archive.created_at, remote_archive.archive_id)
            {
                (local_archive.archive_id, local_archive.created_at)
            } else {
                (remote_archive.archive_id, remote_archive.created_at)
            };
            let events = merged_events(local_archive, remote_archive)?;
            let archive = Archive {
                schema_version: ARCHIVE_SCHEMA_VERSION,
                archive_id,
                created_at,
                events,
            };
            ChronicleView::build(&archive)?;
            let merged = archive != *local_archive || archive != *remote_archive;
            Ok(MergeOutcome {
                archive: Some(archive),
                merged,
            })
        }
    }
}

fn merged_events(local: &Archive, remote: &Archive) -> Result<Vec<ChronicleEvent>, SillokError> {
    let mut events = Vec::with_capacity(local.events.len().saturating_add(remote.events.len()));
    let mut by_event_id = HashMap::with_capacity(events.capacity());
    for event in local.events.iter().chain(remote.events.iter()) {
        match by_event_id.get(&event.event_id) {
            Some(index) => {
                let Some(existing) = events.get(*index) else {
                    return Err(SillokError::new(
                        "sync_merge_conflict",
                        "event index disappeared during merge",
                    ));
                };
                if existing != event {
                    return Err(SillokError::new(
                        "sync_merge_conflict",
                        format!("event id `{}` has conflicting payloads", event.event_id),
                    ));
                }
            }
            None => {
                by_event_id.insert(event.event_id, events.len());
                events.push(event.clone());
            }
        }
    }
    order_events(events)
}

fn order_events(events: Vec<ChronicleEvent>) -> Result<Vec<ChronicleEvent>, SillokError> {
    let mut creation_index = HashMap::with_capacity(events.len());
    for (index, event) in events.iter().enumerate() {
        let Some(record_id) = created_record_id(event) else {
            continue;
        };
        if creation_index.insert(record_id, index).is_some() {
            return Err(SillokError::new(
                "sync_merge_conflict",
                format!("record `{record_id}` has multiple creation events"),
            ));
        }
    }

    let mut dependents = vec![Vec::new(); events.len()];
    let mut indegree = vec![0usize; events.len()];
    for (index, event) in events.iter().enumerate() {
        let mut seen = HashSet::new();
        for record_id in dependency_record_ids(event) {
            let Some(dependency_index) = creation_index.get(&record_id).copied() else {
                continue;
            };
            if dependency_index == index || !seen.insert(dependency_index) {
                continue;
            }
            dependents[dependency_index].push(index);
            indegree[index] = indegree[index].saturating_add(1);
        }
    }

    let mut ready = BTreeSet::new();
    for (index, count) in indegree.iter().enumerate() {
        if *count == 0 {
            ready.insert(sort_key(&events[index], index));
        }
    }

    let mut ordered = Vec::with_capacity(events.len());
    while let Some(key) = ready.pop_first() {
        let index = key.4;
        ordered.push(events[index].clone());
        for dependent in &dependents[index] {
            indegree[*dependent] = indegree[*dependent].saturating_sub(1);
            if indegree[*dependent] == 0 {
                ready.insert(sort_key(&events[*dependent], *dependent));
            }
        }
    }

    if ordered.len() == events.len() {
        Ok(ordered)
    } else {
        Err(SillokError::new(
            "sync_merge_conflict",
            "event dependency graph could not be topologically ordered",
        ))
    }
}

fn sort_key(
    event: &ChronicleEvent,
    index: usize,
) -> (u8, Timestamp, Timestamp, ChronicleId, usize) {
    (
        event_rank(&event.kind),
        event.recorded_at,
        event.event_at,
        event.event_id,
        index,
    )
}

fn event_rank(kind: &EventKind) -> u8 {
    match kind {
        EventKind::ArchiveInitialized { .. } => 0,
        EventKind::DayOpened { .. } => 1,
        EventKind::ObjectiveAdded { .. } | EventKind::TaskRecorded { .. } => 2,
        EventKind::ObjectiveCompleted { .. }
        | EventKind::TaskAmended { .. }
        | EventKind::TaskRetracted { .. }
        | EventKind::TaskLinked { .. }
        | EventKind::TaskUnlinked { .. } => 3,
    }
}

fn created_record_id(event: &ChronicleEvent) -> Option<ChronicleId> {
    match event.kind {
        EventKind::DayOpened { day_id, .. } => Some(day_id),
        EventKind::ObjectiveAdded { objective_id, .. } => Some(objective_id),
        EventKind::TaskRecorded { task_id, .. } => Some(task_id),
        EventKind::ArchiveInitialized { .. }
        | EventKind::ObjectiveCompleted { .. }
        | EventKind::TaskAmended { .. }
        | EventKind::TaskRetracted { .. }
        | EventKind::TaskLinked { .. }
        | EventKind::TaskUnlinked { .. } => None,
    }
}

fn dependency_record_ids(event: &ChronicleEvent) -> Vec<ChronicleId> {
    match event.kind {
        EventKind::ArchiveInitialized { .. } | EventKind::DayOpened { .. } => Vec::new(),
        EventKind::ObjectiveAdded { day_id, .. } => vec![day_id],
        EventKind::ObjectiveCompleted { objective_id, .. } => vec![objective_id],
        EventKind::TaskRecorded {
            day_id, parent_id, ..
        } => vec![day_id, parent_id],
        EventKind::TaskAmended { record_id, .. } => vec![record_id],
        EventKind::TaskRetracted { record_id, .. } => vec![record_id],
        EventKind::TaskLinked {
            child_id,
            parent_id,
        } => vec![child_id, parent_id],
        EventKind::TaskUnlinked { child_id } => vec![child_id],
    }
}
