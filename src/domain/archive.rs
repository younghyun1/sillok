use bitcode::{Decode, Encode};
use serde::Serialize;

use crate::domain::event::{ChronicleEvent, EventKind, WorkContext};
use crate::domain::id::ChronicleId;
use crate::domain::time::Timestamp;

pub const ARCHIVE_SCHEMA_VERSION: u32 = 1;

/// Append-only persisted archive. Serialization is private to the storage layer.
#[derive(Debug, Clone, Serialize, Encode, Decode)]
pub struct Archive {
    pub schema_version: u32,
    pub archive_id: ChronicleId,
    pub created_at: Timestamp,
    pub events: Vec<ChronicleEvent>,
}

impl Archive {
    /// Creates a new archive with an initialization event.
    pub fn new(recorded_at: Timestamp, actor: String, context: WorkContext) -> Self {
        let archive_id = ChronicleId::new_v7();
        let init = ChronicleEvent::new(
            recorded_at,
            recorded_at,
            actor,
            context,
            EventKind::ArchiveInitialized { archive_id },
        );
        Self {
            schema_version: ARCHIVE_SCHEMA_VERSION,
            archive_id,
            created_at: recorded_at,
            events: vec![init],
        }
    }

    /// Appends one event to the archive.
    pub fn push(&mut self, event: ChronicleEvent) {
        self.events.push(event);
    }
}
