//! Command results before rendering, and the output mode.

use serde_json::Value;

/// How the result should appear in compact mode.
#[derive(Debug, Clone)]
pub enum Shape {
    /// A write: print the affected ids, one per line.
    Ids(Vec<String>),
    /// A read: print `data` as one line of JSON.
    Data,
    /// Plain text (for example `guide`).
    Text(String),
    /// The command already streamed its output (for example `export`).
    Streamed,
}

/// A command's result.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub command: &'static str,
    pub shape: Shape,
    /// Payload of the JSON envelope, and of compact reads.
    pub data: Value,
    pub human: Option<String>,
    pub warnings: Vec<String>,
}

impl Outcome {
    /// Result of a write that affected `ids`.
    pub fn write(command: &'static str, ids: Vec<String>, data: Value) -> Self {
        Self {
            command,
            shape: Shape::Ids(ids),
            data,
            human: None,
            warnings: Vec::new(),
        }
    }

    /// Result of a read.
    pub fn read(command: &'static str, data: Value) -> Self {
        Self {
            command,
            shape: Shape::Data,
            data,
            human: None,
            warnings: Vec::new(),
        }
    }

    /// Plain-text result.
    pub fn text(command: &'static str, text: String) -> Self {
        Self {
            command,
            shape: Shape::Text(text.clone()),
            data: serde_json::json!({ "text": text }),
            human: None,
            warnings: Vec::new(),
        }
    }

    /// Result whose output was already written.
    pub fn streamed(command: &'static str, data: Value) -> Self {
        Self {
            command,
            shape: Shape::Streamed,
            data,
            human: None,
            warnings: Vec::new(),
        }
    }

    /// Adds readable text for `--human`.
    pub fn with_human(mut self, text: String) -> Self {
        self.human = Some(text);
        self
    }

    /// Appends warnings.
    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings.extend(warnings);
        self
    }
}

/// Output mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Token-lean default for agents.
    Compact,
    /// Full envelope.
    Json,
    /// Readable text.
    Human,
}

impl Mode {
    /// Flags win; otherwise `SILLOK_OUTPUT`; otherwise compact.
    pub fn resolve(json: bool, human: bool) -> Self {
        if json {
            return Self::Json;
        }
        if human {
            return Self::Human;
        }
        match std::env::var("SILLOK_OUTPUT") {
            Ok(value) => match value.trim() {
                "json" => Self::Json,
                "human" => Self::Human,
                _ => Self::Compact,
            },
            Err(_) => Self::Compact,
        }
    }
}
