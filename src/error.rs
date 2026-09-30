//! The crate's single error type and its stable machine codes.
//!
//! Every variant maps to a `code()` string that agents can branch on; the
//! strings are part of the CLI contract and must not change between 1.x
//! releases. `is_retryable()` tells retry loops which failures are transient.

use rusqlite::ErrorCode;

/// Application error with a stable machine-readable code.
#[derive(Debug, thiserror::Error)]
pub enum SillokError {
    /// Caller input failed validation (text, ids, timestamps, flags).
    #[error("{message}")]
    InvalidInput { code: &'static str, message: String },
    /// The requested record does not exist.
    #[error("record `{0}` does not exist")]
    RecordNotFound(String),
    /// The requested record exists but was retracted.
    #[error("record `{0}` has been retracted")]
    RecordRetracted(String),
    /// The request is well formed but violates a domain rule.
    #[error("{message}")]
    InvalidOperation { code: &'static str, message: String },
    /// Another process holds the store past the wait budget.
    #[error("store is busy: {0}")]
    Busy(String),
    /// SQLite failed for a reason other than contention.
    #[error("sqlite: {0}")]
    Sqlite(rusqlite::Error),
    /// Filesystem or process I/O failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON encoding or decoding failed.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Persisted data has a shape this build does not understand.
    #[error("{message}")]
    Datashape { code: &'static str, message: String },
    /// A 0.9/0.10 artifact could not be decoded.
    #[error("legacy decode failed: {0}")]
    LegacyDecode(String),
    /// Sync configuration or Git execution failed.
    #[error("{message}")]
    Sync { code: &'static str, message: String },
    /// The remote advanced during a push.
    #[error("push rejected: {0}")]
    PushRejected(String),
    /// Command-line parsing failed.
    #[error("{0}")]
    Usage(String),
}

impl SillokError {
    /// Builds an input validation error.
    pub fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidInput {
            code,
            message: message.into(),
        }
    }

    /// Builds a domain rule violation.
    pub fn operation(code: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidOperation {
            code,
            message: message.into(),
        }
    }

    /// Builds an unsupported or corrupt persisted-data error.
    pub fn datashape(code: &'static str, message: impl Into<String>) -> Self {
        Self::Datashape {
            code,
            message: message.into(),
        }
    }

    /// Builds a sync configuration or Git error.
    pub fn sync(code: &'static str, message: impl Into<String>) -> Self {
        Self::Sync {
            code,
            message: message.into(),
        }
    }

    /// Returns the stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput { code, .. }
            | Self::InvalidOperation { code, .. }
            | Self::Datashape { code, .. }
            | Self::Sync { code, .. } => code,
            Self::RecordNotFound(_) => "record_not_found",
            Self::RecordRetracted(_) => "record_retracted",
            Self::Busy(_) => "store_busy",
            Self::Sqlite(_) => "store_error",
            Self::Io(_) => "io_error",
            Self::Json(_) => "json_error",
            Self::LegacyDecode(_) => "legacy_decode_error",
            Self::PushRejected(_) => "sync_push_rejected",
            Self::Usage(_) => "usage",
        }
    }

    /// Returns whether a bounded retry loop should try again.
    ///
    /// Contention and a remote that moved under us are transient; everything
    /// else, including Git authentication failures, stops immediately.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Busy(_) | Self::PushRejected(_))
    }

    /// Returns the process exit code: 2 for usage errors, 1 otherwise.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            _ => 1,
        }
    }
}

impl From<rusqlite::Error> for SillokError {
    fn from(value: rusqlite::Error) -> Self {
        // Contention surfaces as SQLITE_BUSY/SQLITE_LOCKED once busy_timeout
        // expires; classify it so callers can retry or report `store_busy`.
        match value.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy) | Some(ErrorCode::DatabaseLocked) => {
                Self::Busy(value.to_string())
            }
            _ => Self::Sqlite(value),
        }
    }
}

impl From<bitcode::Error> for SillokError {
    fn from(value: bitcode::Error) -> Self {
        Self::LegacyDecode(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::SillokError;

    #[test]
    fn codes_are_stable() {
        assert_eq!(
            SillokError::invalid("invalid_text", "x").code(),
            "invalid_text"
        );
        assert_eq!(
            SillokError::RecordNotFound("a".into()).code(),
            "record_not_found"
        );
        assert_eq!(SillokError::Busy("b".into()).code(), "store_busy");
        assert_eq!(SillokError::Usage("u".into()).exit_code(), 2);
        assert_eq!(SillokError::Busy("b".into()).exit_code(), 1);
    }

    #[test]
    fn only_transient_errors_retry() {
        assert!(SillokError::Busy("b".into()).is_retryable());
        assert!(SillokError::PushRejected("p".into()).is_retryable());
        assert!(!SillokError::sync("sync_git_error", "auth").is_retryable());
    }
}
