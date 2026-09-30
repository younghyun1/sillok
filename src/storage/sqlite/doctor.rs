//! Store validation: SQLite integrity plus a full replay compared field by
//! field against the projection.

use std::collections::HashMap;

use crate::domain::reducer::replay::replay;
use crate::error::SillokError;
use crate::storage::sqlite::open::Store;
use crate::storage::sqlite::{events, records};

/// Mismatches listed before the report truncates; keeps output bounded.
const MAX_REPORTED: usize = 20;

/// Result of `doctor`.
#[derive(Debug, Clone, Default)]
pub struct DoctorReport {
    pub integrity: String,
    pub events: usize,
    pub records: usize,
    pub unknown_events: usize,
    pub warnings: Vec<String>,
    pub mismatches: Vec<String>,
    pub mismatch_count: usize,
}

impl DoctorReport {
    /// Valid when SQLite is intact and the projection equals a replay.
    pub fn valid(&self) -> bool {
        self.integrity == "ok" && self.mismatch_count == 0
    }
}

impl Store {
    /// Validates integrity and projection agreement.
    pub fn doctor(&self) -> Result<DoctorReport, SillokError> {
        let integrity = match self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let stored_events = match events::load_all(&self.conn) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let projection = replay(stored_events.iter());
        let stored = match records::load(&self.conn, "", &[]) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let mut report = DoctorReport {
            integrity,
            events: stored_events.len(),
            records: stored.len(),
            unknown_events: projection.unknown_events,
            warnings: projection.warnings,
            mismatches: Vec::new(),
            mismatch_count: 0,
        };
        let mut by_id: HashMap<_, _> = stored
            .into_iter()
            .map(|record| (record.id, record))
            .collect();
        let mut expected: Vec<_> = projection.records.into_values().collect();
        expected.sort_by_key(|record| (record.created_at, record.id));
        for record in expected {
            match by_id.remove(&record.id) {
                Some(actual) if actual == record => {}
                Some(_) => note(
                    &mut report,
                    format!("record `{}` differs from replay", record.id),
                ),
                None => note(
                    &mut report,
                    format!("record `{}` is missing from the projection", record.id),
                ),
            }
        }
        for id in by_id.keys() {
            note(&mut report, format!("record `{id}` has no events"));
        }
        Ok(report)
    }
}

fn note(report: &mut DoctorReport, message: String) {
    report.mismatch_count += 1;
    if report.mismatches.len() < MAX_REPORTED {
        report.mismatches.push(message);
    }
}
