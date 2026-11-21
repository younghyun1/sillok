//! Diagnostic logging, set up only when something will be logged.
//!
//! Sillok logs on a few rare paths: migrating a 0.x store and cleaning up
//! after sync. Building the JSON subscriber and parsing its filter on every
//! invocation cost more than the command itself for those that never log,
//! so the subscriber is installed on demand. Any path that uses a `tracing`
//! macro must call `ensure` first; events emitted before it are dropped.

use std::sync::Once;

use tracing_subscriber::EnvFilter;

/// Environment variable holding a `tracing` filter; defaults to
/// `sillok=info,warn` so only notable events (such as a migration) log.
pub const LOG_ENV: &str = "SILLOK_LOG";

static INIT: Once = Once::new();

/// Installs the stderr JSON subscriber once per process.
pub fn ensure() {
    INIT.call_once(|| {
        let filter = match EnvFilter::try_from_env(LOG_ENV) {
            Ok(value) => value,
            Err(_) => EnvFilter::new("sillok=info,warn"),
        };
        // Logs go to stderr as JSON; stdout carries command output only.
        match tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .with_ansi(false)
            .with_writer(std::io::stderr)
            .with_env_filter(filter)
            .try_init()
        {
            Ok(()) | Err(_) => {}
        }
    });
}

/// Installs the subscriber at startup when the user asked for logs.
pub fn ensure_if_requested() {
    if std::env::var_os(LOG_ENV).is_some() {
        ensure();
    }
}
