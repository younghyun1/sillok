use std::collections::{HashMap, HashSet};

use crate::domain::id::ChronicleId;
use crate::domain::view::{ChronicleView, DerivedRecord};
use crate::error::SillokError;

/// Rebuilds secondary indexes after all events are reduced.
pub fn rebuild_indexes(view: &mut ChronicleView<'_>) {
    view.by_day.clear();
    view.timeline.clear();
    view.by_tag.clear();
    view.by_context.clear();
    view.by_status.clear();

    for record in view.records.values() {
        view.by_day
            .entry(record.day_id)
            .or_default()
            .push(record.record_id);
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
    sort_indexes(view);
}

/// Validates that parent pointers do not form cycles.
pub fn validate_parent_graph(view: &ChronicleView<'_>) -> Result<(), SillokError> {
    let mut seen = HashSet::with_capacity(view.parent_by_child.len());
    for record_id in view.records.keys() {
        seen.clear();
        let mut current = *record_id;
        while let Some(parent) = view.parent_by_child.get(&current) {
            if !view.records.contains_key(parent) {
                return Err(SillokError::new(
                    "missing_parent",
                    format!("record `{current}` points to missing parent `{parent}`"),
                ));
            }
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

fn sort_indexes(view: &mut ChronicleView<'_>) {
    for bucket in view.by_day.values_mut() {
        sort_record_ids(&view.records, bucket);
    }
    for bucket in view.timeline.values_mut() {
        sort_record_ids(&view.records, bucket);
    }
    for bucket in view.by_tag.values_mut() {
        sort_record_ids(&view.records, bucket);
    }
    for bucket in view.by_context.values_mut() {
        sort_record_ids(&view.records, bucket);
    }
    for bucket in view.by_status.values_mut() {
        sort_record_ids(&view.records, bucket);
    }
}

fn sort_record_ids(records: &HashMap<ChronicleId, DerivedRecord>, ids: &mut [ChronicleId]) {
    ids.sort_by_key(|id| match records.get(id) {
        Some(record) => (record.created_at, record.record_id),
        None => (crate::domain::time::Timestamp::from_millis(0), *id),
    });
}
