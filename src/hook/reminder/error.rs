//! Closed diagnostic categories without hook payload content.

/// Failures name a category only; no command text, path, or session data.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("hook input exceeded its size limit")]
    InputTooLarge,
    #[error("hook input did not close within two seconds")]
    InputTimeout,
    #[error("invalid hook JSON")]
    Json(#[source] serde_json::Error),
    #[error("hook input must be a JSON object with the fields of its event")]
    Shape,
    #[error("hook input has no usable session id")]
    Session,
    #[error("turn ledger I/O failed")]
    Io(#[from] std::io::Error),
}
