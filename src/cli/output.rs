use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{ErrorPayload, SillokError};

/// Successful command response.
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub ok: bool,
    pub command: &'static str,
    pub ids: Value,
    pub data: Value,
    pub warnings: Vec<String>,
}

/// Failed command response.
#[derive(Debug, Serialize)]
pub struct FailureResponse {
    pub ok: bool,
    pub command: &'static str,
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
    pub fn success_response(self) -> SuccessResponse {
        SuccessResponse {
            ok: true,
            command: self.command,
            ids: self.ids,
            data: self.data,
            warnings: self.warnings,
        }
    }
}

/// Prints a command result. JSON is the stable default.
pub fn print_success(outcome: CommandOutcome, human: bool) -> Result<(), SillokError> {
    if human {
        match outcome.human {
            Some(value) => {
                println!("{value}");
                Ok(())
            }
            None => {
                println!("{}", serde_json::to_string_pretty(&outcome.data)?);
                Ok(())
            }
        }
    } else {
        let response = outcome.success_response();
        println!("{}", serde_json::to_string(&response)?);
        Ok(())
    }
}

/// Prints a structured failure response.
pub fn print_failure(command: &'static str, error: &SillokError) -> Result<(), SillokError> {
    let response = FailureResponse {
        ok: false,
        command,
        error: error.payload(),
    };
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}
