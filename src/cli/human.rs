use std::path::Path;

use crate::domain::archive::Archive;
use crate::domain::event::{ChronicleEvent, EventKind, RecordKind, RecordStatus};
use crate::domain::id::ChronicleId;
use crate::domain::time::{DayKey, Timestamp};
use crate::domain::view::{DerivedRecord, RecordTreeNode};

/// Formats `init` output.
pub fn init(archive: &Archive, created: bool, store: &Path) -> String {
    let state = if created { "Created" } else { "Existing" };
    format!(
        "{state} archive\nid: {}\ncreated: {}\nstore: {}",
        archive.archive_id,
        archive.created_at,
        store.display()
    )
}

/// Formats a mutation that returns one record.
pub fn record_action(action: &str, record: &DerivedRecord) -> String {
    let mut output = String::new();
    output.push_str(action);
    output.push('\n');
    push_record_details(&mut output, record, "");
    trim_trailing_newline(output)
}

/// Formats `show` output with record details and event history.
pub fn show(record: &DerivedRecord, events: &[ChronicleEvent]) -> String {
    let mut output = String::new();
    output.push_str("Record\n");
    push_record_details(&mut output, record, "");
    output.push_str("\nEvents\n");
    if events.is_empty() {
        output.push_str("No events.\n");
    } else {
        for event in events {
            push_event(&mut output, event);
        }
    }
    trim_trailing_newline(output)
}

/// Formats a day tree for interactive terminal output.
pub fn day(day_key: &DayKey, record_count: usize, tree: Option<&RecordTreeNode>) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "{} ({}) - {} {}\n",
        day_key.date,
        day_key.timezone,
        record_count,
        pluralize(record_count, "record")
    ));

    match tree {
        Some(root) if !root.children.is_empty() => {
            for child in &root.children {
                push_node(&mut output, child, 0);
            }
        }
        Some(_) | None => output.push_str("No records.\n"),
    }

    trim_trailing_newline(output)
}

/// Formats query/export-style record lists.
pub fn records(title: &str, records: &[DerivedRecord]) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "{title} - {} {}\n",
        records.len(),
        pluralize(records.len(), "record")
    ));
    if records.is_empty() {
        output.push_str("No records.\n");
    } else {
        for record in records {
            push_record_brief(&mut output, record, "");
        }
    }
    trim_trailing_newline(output)
}

/// Formats `tree` output.
pub fn tree(root_id: Option<ChronicleId>, tree: Option<&RecordTreeNode>) -> String {
    let mut output = String::new();
    match (root_id, tree) {
        (Some(id), Some(node)) => {
            output.push_str(&format!("Tree rooted at {id}\n"));
            push_node(&mut output, node, 0);
        }
        _ => output.push_str("Empty tree.\n"),
    }
    trim_trailing_newline(output)
}

/// Formats successful `doctor` output.
pub fn doctor_valid(
    archive: &Archive,
    record_count: usize,
    store: &Path,
    checked_at: Timestamp,
) -> String {
    format!(
        "Archive valid\nchecked: {checked_at}\narchive: {}\ncreated: {}\nevents: {}\nrecords: {}\nstore: {}",
        archive.archive_id,
        archive.created_at,
        archive.events.len(),
        record_count,
        store.display()
    )
}

/// Formats missing-archive `doctor` output.
pub fn doctor_missing(store: &Path, checked_at: Timestamp) -> String {
    format!(
        "Archive missing; no corruption found\nchecked: {checked_at}\nstore: {}",
        store.display()
    )
}

/// Formats invalid-archive `doctor` output.
pub fn doctor_invalid(
    error: &crate::error::SillokError,
    store: &Path,
    checked_at: Timestamp,
) -> String {
    format!(
        "Archive invalid\nchecked: {checked_at}\nerror: {error}\nstore: {}",
        store.display()
    )
}

/// Formats `truncate` output.
pub fn truncate(archive: &Archive, backup: Option<&Path>, store: &Path) -> String {
    let backup_value = match backup {
        Some(path) => path.display().to_string(),
        None => "none".to_string(),
    };
    format!(
        "Truncated archive\narchive: {}\ncreated: {}\nbackup: {}\nstore: {}",
        archive.archive_id,
        archive.created_at,
        backup_value,
        store.display()
    )
}

fn push_node(output: &mut String, node: &RecordTreeNode, depth: usize) {
    push_record_brief(output, &node.record, &"  ".repeat(depth));
    for child in &node.children {
        push_node(output, child, depth + 1);
    }
}

fn push_record_brief(output: &mut String, record: &DerivedRecord, indent: &str) {
    output.push_str(&format!(
        "{}- [{} {}] {} ({})\n",
        indent,
        status_label(record.status),
        kind_label(record.kind),
        record.text,
        record.created_at
    ));
    output.push_str(&format!("{}  id: {}\n", indent, record.record_id));
    if record.updated_at != record.created_at {
        output.push_str(&format!("{}  updated: {}\n", indent, record.updated_at));
    }
    if let Some(purpose) = &record.purpose {
        output.push_str(&format!("{}  note: {}\n", indent, purpose));
    }
    if !record.tags.is_empty() {
        output.push_str(&format!("{}  tags: {}\n", indent, record.tags.join(", ")));
    }
}

fn push_record_details(output: &mut String, record: &DerivedRecord, indent: &str) {
    output.push_str(&format!(
        "{}{} {}: {}\n",
        indent,
        status_label(record.status),
        kind_label(record.kind),
        record.text
    ));
    output.push_str(&format!("{}id: {}\n", indent, record.record_id));
    output.push_str(&format!("{}created: {}\n", indent, record.created_at));
    output.push_str(&format!("{}updated: {}\n", indent, record.updated_at));
    output.push_str(&format!("{}day: {}\n", indent, record.day_id));
    if let Some(parent_id) = record.parent_id {
        output.push_str(&format!("{}parent: {}\n", indent, parent_id));
    }
    if let Some(purpose) = &record.purpose {
        output.push_str(&format!("{}note: {}\n", indent, purpose));
    }
    if !record.tags.is_empty() {
        output.push_str(&format!("{}tags: {}\n", indent, record.tags.join(", ")));
    }
}

fn push_event(output: &mut String, event: &ChronicleEvent) {
    output.push_str(&format!(
        "- {} at {} (recorded {})\n",
        event_label(&event.kind),
        event.event_at,
        event.recorded_at
    ));
    output.push_str(&format!("  id: {}\n", event.event_id));
}

fn event_label(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::ArchiveInitialized { .. } => "archive initialized",
        EventKind::DayOpened { .. } => "day opened",
        EventKind::ObjectiveAdded { .. } => "objective added",
        EventKind::ObjectiveCompleted { .. } => "objective completed",
        EventKind::TaskRecorded { .. } => "task recorded",
        EventKind::TaskAmended { .. } => "task amended",
        EventKind::TaskRetracted { .. } => "task retracted",
        EventKind::TaskLinked { .. } => "task linked",
        EventKind::TaskUnlinked { .. } => "task unlinked",
    }
}

fn kind_label(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Day => "day",
        RecordKind::Task => "task",
        RecordKind::Objective => "objective",
    }
}

fn status_label(status: RecordStatus) -> &'static str {
    match status {
        RecordStatus::Open => "open",
        RecordStatus::Active => "active",
        RecordStatus::Blocked => "blocked",
        RecordStatus::Completed => "completed",
        RecordStatus::Retracted => "retracted",
    }
}

fn pluralize(count: usize, noun: &'static str) -> String {
    match count {
        1 => noun.to_string(),
        _ => format!("{noun}s"),
    }
}

fn trim_trailing_newline(mut value: String) -> String {
    if value.ends_with('\n') {
        value.pop();
    }
    value
}
