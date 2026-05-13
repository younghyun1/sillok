use crate::domain::event::{RecordKind, RecordStatus};
use crate::domain::time::DayKey;
use crate::domain::view::{DerivedRecord, RecordTreeNode};

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

fn push_node(output: &mut String, node: &RecordTreeNode, depth: usize) {
    push_record(output, &node.record, depth);
    for child in &node.children {
        push_node(output, child, depth + 1);
    }
}

fn push_record(output: &mut String, record: &DerivedRecord, depth: usize) {
    let indent = "  ".repeat(depth);
    output.push_str(&format!(
        "{}- [{} {}] {}\n",
        indent,
        status_label(record.status),
        kind_label(record.kind),
        record.text
    ));
    output.push_str(&format!("{}  id: {}\n", indent, record.record_id));

    if let Some(purpose) = &record.purpose {
        output.push_str(&format!("{}  note: {}\n", indent, purpose));
    }
    if !record.tags.is_empty() {
        output.push_str(&format!("{}  tags: {}\n", indent, record.tags.join(", ")));
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
