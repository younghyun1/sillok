//! JSON shapes of records and trees.
//!
//! Compact by design: absent optional fields are omitted and the work
//! context appears only with `--full`, because agents pay for every token.

use serde_json::{Value, json};

use crate::domain::record::Record;
use crate::domain::tree::TreeNode;

/// One record.
pub fn record(record: &Record, full: bool) -> Value {
    let mut value = match serde_json::to_value(record) {
        Ok(value) => value,
        Err(_) => json!({ "id": record.id }),
    };
    if full && let Value::Object(map) = &mut value {
        match serde_json::to_value(&record.context) {
            Ok(context) => {
                map.insert("context".to_string(), context);
            }
            Err(_) => {}
        }
    }
    value
}

/// A list of records.
pub fn records(list: &[Record], full: bool) -> Value {
    Value::Array(list.iter().map(|item| record(item, full)).collect())
}

/// A forest of tree nodes.
pub fn forest(nodes: &[TreeNode], full: bool) -> Value {
    Value::Array(nodes.iter().map(|node| tree(node, full)).collect())
}

fn tree(node: &TreeNode, full: bool) -> Value {
    let mut value = record(&node.record, full);
    if let Value::Object(map) = &mut value {
        if !node.activity.is_empty() {
            map.insert("activity".to_string(), json!(node.activity));
        }
        if !node.children.is_empty() {
            map.insert("children".to_string(), forest(&node.children, full));
        }
    }
    value
}
