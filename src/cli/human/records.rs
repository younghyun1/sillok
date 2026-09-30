//! Readable text for records, trees, and lists.

use crate::domain::event::envelope::Event;
use crate::domain::record::Record;
use crate::domain::tree::TreeNode;
use crate::domain::zone::Zone;

/// One-line summary: `[status kind] text  (id, local time)`.
pub fn line(record: &Record, zone: &Zone) -> String {
    let mut out = format!(
        "[{} {}] {}  ({}, {})",
        record.status.as_str(),
        record.kind.as_str(),
        record.text,
        record.id,
        zone.format_human(record.created_at)
    );
    if !record.tags.is_empty() {
        out.push_str(&format!("  #{}", record.tags.join(" #")));
    }
    out
}

/// Indented forest; activity follows each touched record.
pub fn forest(title: &str, nodes: &[TreeNode], zone: &Zone) -> String {
    let mut out = format!("{title}\n");
    if nodes.is_empty() {
        out.push_str("No records.");
        return out;
    }
    for node in nodes {
        push_node(&mut out, node, 0, zone);
    }
    trim(out)
}

/// Flat list with a count header.
pub fn list(title: &str, records: &[Record], zone: &Zone) -> String {
    let mut out = format!("{title} ({})\n", records.len());
    for record in records {
        out.push_str(&format!("- {}", line(record, zone)));
        out.push('\n');
    }
    trim(out)
}

/// Record details and history.
pub fn show(record: &Record, events: &[Event], zone: &Zone) -> String {
    let mut out = line(record, zone);
    out.push('\n');
    if let Some(parent) = record.parent {
        out.push_str(&format!("parent: {parent}"));
        out.push('\n');
    }
    if let Some(purpose) = &record.purpose {
        out.push_str(&format!("purpose: {purpose}"));
        out.push('\n');
    }
    if let Some(note) = &record.note {
        out.push_str(&format!("note: {note}"));
        out.push('\n');
    }
    if let Some(reason) = &record.retraction_reason {
        out.push_str(&format!("retracted: {reason}"));
        out.push('\n');
    }
    out.push_str(&format!(
        "updated: {}",
        zone.format_human(record.updated_at)
    ));
    out.push('\n');
    out.push_str("history:\n");
    for event in events {
        out.push_str(&format!(
            "- {} {} by {}",
            zone.format_human(event.body.event_at),
            event.body.kind.activity(),
            event.body.actor
        ));
        out.push('\n');
    }
    trim(out)
}

fn push_node(out: &mut String, node: &TreeNode, depth: usize, zone: &Zone) {
    let indent = "  ".repeat(depth);
    out.push_str(&format!("{indent}- {}", line(&node.record, zone)));
    if !node.activity.is_empty() {
        out.push_str(&format!("  <{}>", node.activity.join(", ")));
    }
    out.push('\n');
    for child in &node.children {
        push_node(out, child, depth + 1, zone);
    }
}

fn trim(mut value: String) -> String {
    while value.ends_with('\n') {
        value.pop();
    }
    value
}
