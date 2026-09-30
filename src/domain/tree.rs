//! Builds display forests from a set of records.

use std::collections::{HashMap, HashSet};

use crate::domain::id::RecordId;
use crate::domain::record::Record;

/// One record with the children shown beneath it.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub record: Record,
    /// What happened to the record in the viewed window; empty when the
    /// record is only present to give its descendants a place in the tree.
    pub activity: Vec<&'static str>,
    pub children: Vec<TreeNode>,
}

/// Arranges records into a forest. A record is a root when it has no parent
/// or its parent is not in the set. Siblings are ordered by creation time.
pub fn build_forest(
    records: Vec<Record>,
    mut activity: HashMap<RecordId, Vec<&'static str>>,
) -> Vec<TreeNode> {
    let present: HashSet<RecordId> = records.iter().map(|record| record.id).collect();
    let mut children: HashMap<RecordId, Vec<Record>> = HashMap::new();
    let mut roots = Vec::new();
    for record in records {
        match record.parent {
            Some(parent) if present.contains(&parent) && parent != record.id => {
                children.entry(parent).or_default().push(record);
            }
            _ => roots.push(record),
        }
    }
    roots.sort_by_key(|record| (record.created_at, record.id));
    let mut visiting = HashSet::new();
    roots
        .into_iter()
        .map(|record| attach(record, &mut children, &mut activity, &mut visiting))
        .collect()
}

fn attach(
    record: Record,
    children: &mut HashMap<RecordId, Vec<Record>>,
    activity: &mut HashMap<RecordId, Vec<&'static str>>,
    visiting: &mut HashSet<RecordId>,
) -> TreeNode {
    visiting.insert(record.id);
    let mut kids = match children.remove(&record.id) {
        Some(value) => value,
        None => Vec::new(),
    };
    kids.sort_by_key(|child| (child.created_at, child.id));
    let mut nodes = Vec::with_capacity(kids.len());
    for child in kids {
        if !visiting.contains(&child.id) {
            nodes.push(attach(child, children, activity, visiting));
        }
    }
    let labels = match activity.remove(&record.id) {
        Some(value) => value,
        None => Vec::new(),
    };
    TreeNode {
        record,
        activity: labels,
        children: nodes,
    }
}
