//! Projection rebuilds and bulk event merges.
//!
//! The event table is never rewritten wholesale, so a rebuild only deletes
//! and replays projection rows inside one transaction. There is no file swap
//! and no backup to accumulate.

use rusqlite::{Connection, TransactionBehavior};

use crate::domain::event::envelope::Event;
use crate::domain::reducer::replay::replay;
use crate::error::SillokError;
use crate::storage::sqlite::open::Store;
use crate::storage::sqlite::{events, records};

/// What a rebuild derived.
#[derive(Debug, Clone, Default)]
pub struct RebuildSummary {
    pub events: usize,
    pub records: usize,
    pub unknown_events: usize,
    pub warnings: Vec<String>,
}

/// What a merge changed.
#[derive(Debug, Clone, Default)]
pub struct MergeSummary {
    pub added: usize,
    pub replaced: usize,
    pub rebuild: Option<RebuildSummary>,
}

/// Replaces every projection row with a replay of the event table.
pub fn rebuild(conn: &Connection) -> Result<RebuildSummary, SillokError> {
    let events = match events::load_all(conn) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let projection = replay(events.iter());
    // record_tag rows cascade; parent FKs are deferred until commit.
    if let Err(error) = conn.execute("DELETE FROM record", []) {
        return Err(error.into());
    }
    let mut ordered: Vec<_> = projection.records.values().collect();
    ordered.sort_by_key(|record| (record.created_at, record.id));
    for record in &ordered {
        if let Err(error) = records::upsert(conn, record) {
            return Err(error);
        }
    }
    Ok(RebuildSummary {
        events: events.len(),
        records: ordered.len(),
        unknown_events: projection.unknown_events,
        warnings: projection.warnings,
    })
}

impl Store {
    /// Rebuilds the projection in one transaction.
    pub fn rebuild_projection(&mut self) -> Result<RebuildSummary, SillokError> {
        let tx = match self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
        {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let summary = match rebuild(&tx) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        match tx.commit() {
            Ok(()) => Ok(summary),
            Err(error) => Err(error.into()),
        }
    }

    /// Inserts new events, overwrites `replacements` (same id, winning bytes),
    /// and rebuilds the projection when anything changed.
    pub fn merge(
        &mut self,
        additions: &[Event],
        replacements: &[Event],
    ) -> Result<MergeSummary, SillokError> {
        let tx = match self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
        {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let mut summary = MergeSummary::default();
        for event in additions {
            match events::insert(&tx, event) {
                Ok(true) => summary.added += 1,
                Ok(false) => {}
                Err(error) => return Err(error),
            }
        }
        for event in replacements {
            if let Err(error) = events::replace(&tx, event) {
                return Err(error);
            }
            summary.replaced += 1;
        }
        if summary.added > 0 || summary.replaced > 0 {
            match rebuild(&tx) {
                Ok(value) => summary.rebuild = Some(value),
                Err(error) => return Err(error),
            }
        }
        match tx.commit() {
            Ok(()) => Ok(summary),
            Err(error) => Err(error.into()),
        }
    }
}
