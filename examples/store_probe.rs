//! Store latency probe at 50k records.
//!
//! `cargo run --example store_probe [-- <dir>]` builds a store of 50k task
//! events, then times one append, a day view, a filtered query, and a full
//! doctor replay. Numbers from a dev build are only comparable to other dev
//! builds.

use std::path::PathBuf;
use std::time::Instant;

use sillok::domain::event::context::WorkContext;
use sillok::domain::event::envelope::{Event, EventBody};
use sillok::domain::event::kind::EventKind;
use sillok::domain::id::{EventId, RecordId};
use sillok::domain::record::RecordStatus;
use sillok::domain::time::Timestamp;
use sillok::error::SillokError;
use sillok::storage::sqlite::open::{OpenMode, Store};
use sillok::storage::sqlite::reads::RecordFilter;
use sillok::storage::sqlite::writes::Stamp;

const RECORDS: usize = 50_000;
/// 2026-05-13T00:00:00Z; one event per minute from there.
const BASE_MS: i64 = 1_778_630_400_000;

fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("probe failed: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), SillokError> {
    let dir = match std::env::args().nth(1) {
        Some(value) => PathBuf::from(value),
        None => std::env::temp_dir().join("sillok-store-probe"),
    };
    if dir.exists()
        && let Err(error) = std::fs::remove_dir_all(&dir)
    {
        return Err(error.into());
    }
    let path = dir.join("sillok.db");
    let mut store = match Store::open(&path, OpenMode::Create) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return Err(SillokError::datashape(
                "store_missing",
                "store was not created",
            ));
        }
        Err(error) => return Err(error),
    };
    let context = WorkContext {
        cwd: Some("/probe".into()),
        git_root: Some("/probe".into()),
        ..WorkContext::default()
    };
    let mut events = Vec::with_capacity(RECORDS);
    for index in 0..RECORDS {
        let at = Timestamp::from_millis(BASE_MS + index as i64 * 60_000);
        let body = EventBody {
            event_id: EventId::new_v7(),
            event_at: at,
            recorded_at: at,
            actor: "probe".into(),
            context: context.clone(),
            kind: EventKind::TaskRecorded {
                record_id: RecordId::new_v7(),
                parent_id: None,
                text: format!("probe task {index}"),
                purpose: None,
                tags: vec![if index % 2 == 0 { "even" } else { "odd" }.to_string()],
                status: if index % 3 == 0 {
                    RecordStatus::Completed
                } else {
                    RecordStatus::Active
                },
            },
        };
        match Event::from_body(body) {
            Ok(event) => events.push(event),
            Err(error) => return Err(error),
        }
    }
    let started = Instant::now();
    if let Err(error) = store.merge(&events, &[]) {
        return Err(error);
    }
    println!(
        "bulk_import_ms={:.1}",
        started.elapsed().as_secs_f64() * 1e3
    );
    drop(events);

    let started = Instant::now();
    let appended = store.append(
        Stamp {
            event_at: Timestamp::now(),
            recorded_at: Timestamp::now(),
            backfilled: false,
            actor: "probe".into(),
            context: context.clone(),
        },
        EventKind::TaskRecorded {
            record_id: RecordId::new_v7(),
            parent_id: None,
            text: "one more".into(),
            purpose: None,
            tags: vec![],
            status: RecordStatus::Completed,
        },
    );
    if let Err(error) = appended {
        return Err(error);
    }
    println!("append_one_ms={:.2}", started.elapsed().as_secs_f64() * 1e3);

    let day_start = Timestamp::from_millis(BASE_MS + 10 * 86_400_000);
    let day_end = Timestamp::from_millis(day_start.as_millis() + 86_400_000);
    let started = Instant::now();
    let day = match store.day(day_start, day_end) {
        Ok((records, _)) => records.len(),
        Err(error) => return Err(error),
    };
    println!(
        "day_records={day} day_ms={:.2}",
        started.elapsed().as_secs_f64() * 1e3
    );

    let started = Instant::now();
    let filter = RecordFilter {
        tags: vec!["even".into()],
        status: Some(RecordStatus::Completed),
        limit: 100,
        ..RecordFilter::default()
    };
    let found = match store.query(&filter) {
        Ok(records) => records.len(),
        Err(error) => return Err(error),
    };
    println!(
        "query_records={found} query_ms={:.2}",
        started.elapsed().as_secs_f64() * 1e3
    );

    let started = Instant::now();
    let report = match store.doctor() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    println!(
        "doctor_valid={} doctor_events={} doctor_ms={:.1}",
        report.valid(),
        report.events,
        started.elapsed().as_secs_f64() * 1e3
    );
    let bytes = match std::fs::metadata(&path) {
        Ok(meta) => meta.len(),
        Err(error) => return Err(error.into()),
    };
    println!("store_mib={:.2}", bytes as f64 / 1_048_576.0);
    Ok(())
}
