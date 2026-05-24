use serde::Serialize;
use serde_json::{Value, json};

use crate::domain::time::Timestamp;
use crate::error::{ErrorPayload, SillokError};

/// CLI output rendering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Compact machine output optimized for agent token usage.
    Compact,
    /// Full JSON response envelope.
    Json,
    /// Human-readable command summaries.
    Human,
}

/// Successful command response.
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub ok: bool,
    pub command: &'static str,
    pub generated_at: Timestamp,
    pub ids: Value,
    pub data: Value,
    pub warnings: Vec<String>,
}

/// Failed command response.
#[derive(Debug, Serialize)]
pub struct FailureResponse {
    pub ok: bool,
    pub command: &'static str,
    pub generated_at: Timestamp,
    pub error: ErrorPayload,
}

/// Internal command outcome before printing.
#[derive(Debug)]
pub struct CommandOutcome {
    pub command: &'static str,
    pub ids: Value,
    pub data: Value,
    pub warnings: Vec<String>,
    pub human: Option<String>,
}

impl CommandOutcome {
    /// Builds an outcome with empty ids and warnings.
    pub fn new(command: &'static str, data: Value) -> Self {
        Self {
            command,
            ids: json!({}),
            data,
            warnings: Vec::new(),
            human: None,
        }
    }

    /// Adds ids to the outcome.
    pub fn with_ids(mut self, ids: Value) -> Self {
        self.ids = ids;
        self
    }

    /// Adds warnings to the outcome.
    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }

    /// Adds human-readable output.
    pub fn with_human(mut self, human: String) -> Self {
        self.human = Some(human);
        self
    }

    /// Converts into the public success response.
    pub fn success_response(self, generated_at: Timestamp) -> SuccessResponse {
        SuccessResponse {
            ok: true,
            command: self.command,
            generated_at,
            ids: self.ids,
            data: self.data,
            warnings: self.warnings,
        }
    }
}

/// Prints a command result.
pub fn print_success(outcome: CommandOutcome, mode: OutputMode) -> Result<(), SillokError> {
    match mode {
        OutputMode::Human => match outcome.human {
            Some(value) => {
                println!("{value}");
                Ok(())
            }
            None => {
                println!("{}", serde_json::to_string_pretty(&outcome.data)?);
                Ok(())
            }
        },
        OutputMode::Json => {
            let response = outcome.success_response(Timestamp::now());
            println!("{}", serde_json::to_string(&response)?);
            Ok(())
        }
        OutputMode::Compact => {
            let response = compact_success_value(outcome);
            println!("{}", serde_json::to_string(&response)?);
            Ok(())
        }
    }
}

/// Prints a failure response.
pub fn print_failure(
    command: &'static str,
    error: &SillokError,
    mode: OutputMode,
) -> Result<(), SillokError> {
    match mode {
        OutputMode::Human => {
            println!("{error}");
            Ok(())
        }
        OutputMode::Json => {
            let response = FailureResponse {
                ok: false,
                command,
                generated_at: Timestamp::now(),
                error: error.payload(),
            };
            println!("{}", serde_json::to_string(&response)?);
            Ok(())
        }
        OutputMode::Compact => {
            let payload = error.payload();
            let response = json!({
                "error": payload.code,
                "message": payload.message,
            });
            println!("{}", serde_json::to_string(&response)?);
            Ok(())
        }
    }
}

fn compact_success_value(outcome: CommandOutcome) -> Value {
    let CommandOutcome {
        command,
        ids,
        data,
        warnings,
        human: _,
    } = outcome;
    let mut value = if should_compact_to_ids(command, &ids) {
        ids
    } else {
        data
    };
    append_warnings(&mut value, warnings);
    value
}

fn should_compact_to_ids(command: &'static str, ids: &Value) -> bool {
    if !has_object_fields(ids) {
        return false;
    }
    matches!(
        command,
        "init" | "note" | "objective" | "amend" | "retract" | "truncate"
    )
}

fn has_object_fields(value: &Value) -> bool {
    match value {
        Value::Object(values) => !values.is_empty(),
        _ => false,
    }
}

fn append_warnings(value: &mut Value, warnings: Vec<String>) {
    if warnings.is_empty() {
        return;
    }
    match value {
        Value::Object(values) => {
            values.insert("warnings".to_string(), json!(warnings));
        }
        _ => {
            let data = std::mem::replace(value, Value::Null);
            *value = json!({
                "data": data,
                "warnings": warnings,
            });
        }
    }
}
