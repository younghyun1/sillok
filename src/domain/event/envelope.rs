//! Event envelope and its canonical JSON bytes.
//!
//! An event is serialized exactly once, when it is created. Those bytes are
//! what the store keeps and sync compares, so fields a newer version added
//! survive a round trip through an older binary untouched.

use serde::{Deserialize, Serialize};

use crate::domain::event::context::WorkContext;
use crate::domain::event::kind::EventKind;
use crate::domain::id::EventId;
use crate::domain::time::Timestamp;
use crate::error::SillokError;

/// Longest event line accepted from any source; bounds memory per line.
pub const MAX_EVENT_BYTES: usize = 1 << 20;

/// Typed view of an event's JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBody {
    pub event_id: EventId,
    /// When the work happened (`--at` backfills move this).
    pub event_at: Timestamp,
    /// When the event was written; orders last-writer-wins updates.
    pub recorded_at: Timestamp,
    pub actor: String,
    #[serde(default)]
    pub context: WorkContext,
    pub kind: EventKind,
}

/// A parsed event together with its canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub body: EventBody,
    pub raw: String,
}

/// The two fields sync needs to place a line without a full parse.
#[derive(Debug, Deserialize)]
pub struct EventKey {
    pub event_id: EventId,
    pub recorded_at: Timestamp,
}

impl Event {
    /// Creates a new event with a fresh id and serializes it once.
    pub fn create(
        event_at: Timestamp,
        recorded_at: Timestamp,
        actor: String,
        context: WorkContext,
        kind: EventKind,
    ) -> Result<Self, SillokError> {
        Self::from_body(EventBody {
            event_id: EventId::new_v7(),
            event_at,
            recorded_at,
            actor,
            context,
            kind,
        })
    }

    /// Serializes a body into canonical bytes.
    pub fn from_body(body: EventBody) -> Result<Self, SillokError> {
        match serde_json::to_string(&body) {
            Ok(raw) => Ok(Self { body, raw }),
            Err(error) => Err(error.into()),
        }
    }

    /// Parses stored or synced bytes, keeping them verbatim.
    pub fn parse(raw: String) -> Result<Self, SillokError> {
        if raw.len() > MAX_EVENT_BYTES {
            return Err(SillokError::datashape(
                "invalid_datashape",
                format!(
                    "event line is {} bytes; limit is {MAX_EVENT_BYTES}",
                    raw.len()
                ),
            ));
        }
        match serde_json::from_str::<EventBody>(&raw) {
            Ok(body) => Ok(Self { body, raw }),
            Err(error) => Err(SillokError::datashape(
                "invalid_event",
                format!("event does not parse: {error}"),
            )),
        }
    }

    /// Event id shortcut.
    pub fn id(&self) -> EventId {
        self.body.event_id
    }

    /// Replay order for last-writer-wins: recorded time, then id.
    pub fn order_key(&self) -> (Timestamp, EventId) {
        (self.body.recorded_at, self.body.event_id)
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, EventKind};
    use crate::domain::event::context::WorkContext;
    use crate::domain::id::RecordId;
    use crate::domain::record::RecordStatus;
    use crate::domain::time::Timestamp;

    fn sample() -> Result<Event, crate::error::SillokError> {
        Event::create(
            Timestamp::from_millis(1_000),
            Timestamp::from_millis(2_000),
            "agent".into(),
            WorkContext::default(),
            EventKind::TaskRecorded {
                record_id: RecordId::new_v7(),
                parent_id: None,
                text: "hello".into(),
                purpose: None,
                tags: vec![],
                status: RecordStatus::Completed,
            },
        )
    }

    #[test]
    fn parse_keeps_bytes_and_body() -> Result<(), crate::error::SillokError> {
        let event = match sample() {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let parsed = match Event::parse(event.raw.clone()) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        assert_eq!(parsed, event);
        Ok(())
    }

    #[test]
    fn unknown_types_and_fields_survive() {
        let raw = r#"{"event_id":"01a0f01e-f6f0-7091-a38c-09f1b8f3c285","event_at":"2026-09-30T00:00:00.000Z","recorded_at":"2026-09-30T00:00:00.000Z","actor":"future","kind":{"type":"record_starred","record_id":"01a0f01e-f6f0-7091-a38c-09f1b8f3c286"},"new_field":1}"#;
        let parsed = Event::parse(raw.to_string());
        assert!(matches!(&parsed, Ok(event) if event.body.kind == EventKind::Unknown));
        assert!(matches!(parsed, Ok(event) if event.raw == raw));
    }

    #[test]
    fn empty_optional_fields_are_omitted() -> Result<(), crate::error::SillokError> {
        let event = match sample() {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        assert!(!event.raw.contains("purpose"));
        assert!(!event.raw.contains("\"cwd\""));
        assert!(event.raw.contains("\"type\":\"task_recorded\""));
        Ok(())
    }
}
