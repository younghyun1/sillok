use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

/// Stable error payload used by the JSON CLI contract.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorPayload {
    pub code: &'static str,
    pub message: String,
}

/// Application error with a stable machine code and readable message.
#[derive(Debug, Clone)]
pub struct SillokError {
    code: &'static str,
    message: String,
}

impl SillokError {
    /// Builds an application error.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the stable machine-readable code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Converts the error into its JSON response shape.
    pub fn payload(&self) -> ErrorPayload {
        ErrorPayload {
            code: self.code,
            message: self.message.clone(),
        }
    }
}

impl Display for SillokError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl Error for SillokError {}

impl From<std::io::Error> for SillokError {
    fn from(value: std::io::Error) -> Self {
        Self::new("io_error", value.to_string())
    }
}

impl From<serde_json::Error> for SillokError {
    fn from(value: serde_json::Error) -> Self {
        Self::new("json_error", value.to_string())
    }
}

impl From<bitcode::Error> for SillokError {
    fn from(value: bitcode::Error) -> Self {
        Self::new("archive_decode_error", value.to_string())
    }
}
