//! Store maintenance: init, doctor, reset, export, guide.

use std::io::{BufWriter, ErrorKind, Write};

use rusqlite::TransactionBehavior;
use serde_json::json;

use crate::cli::args::read::{ExportArgs, ExportFormat};
use crate::cli::output::outcome::Outcome;
use crate::cli::output::views;
use crate::commands::ctx::Ctx;
use crate::domain::id::ArchiveId;
use crate::error::SillokError;
use crate::storage::path::with_suffix;
use crate::storage::sqlite::events;
use crate::storage::sqlite::open::{StoreInfo, write_info};
use crate::storage::sqlite::reads::RecordFilter;

/// The agent guide printed by `sillok guide`.
pub const GUIDE: &str = include_str!("../guide.md");

/// `init`.
pub fn init(ctx: &mut Ctx) -> Result<Outcome, SillokError> {
    let store = match ctx.open_or_create() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    match store.info() {
        Ok(info) => Ok(Outcome::write(
            "init",
            vec![info.archive_id.to_string()],
            json!({
                "archive_id": info.archive_id,
                "created_at": info.created_at,
                "store": store.path().display().to_string(),
            }),
        )
        .with_warnings(std::mem::take(&mut ctx.warnings))),
        Err(error) => Err(error),
    }
}

/// `doctor`.
pub fn doctor(ctx: &mut Ctx, repair: bool) -> Result<Outcome, SillokError> {
    let mut store = match ctx.open() {
        Ok(Some(value)) => value,
        Ok(None) => {
            return Ok(Outcome::read(
                "doctor",
                json!({ "valid": true, "missing": true }),
            ));
        }
        Err(error) => return Err(error),
    };
    let repaired = match repair {
        true => match store.rebuild_projection() {
            Ok(summary) => Some(summary.records),
            Err(error) => return Err(error),
        },
        false => None,
    };
    let report = match store.doctor() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let human = format!(
        "{}: {} events, {} records, integrity {}{}",
        if report.valid() { "valid" } else { "INVALID" },
        report.events,
        report.records,
        report.integrity,
        if report.mismatch_count > 0 {
            format!(
                ", {} mismatches (run `sillok doctor --repair`)",
                report.mismatch_count
            )
        } else {
            String::new()
        }
    );
    Ok(Outcome::read(
        "doctor",
        json!({
            "valid": report.valid(),
            "integrity": report.integrity,
            "events": report.events,
            "records": report.records,
            "unknown_events": report.unknown_events,
            "mismatch_count": report.mismatch_count,
            "mismatches": report.mismatches,
            "replay_notes": report.warnings,
            "repaired": repaired,
        }),
    )
    .with_human(human)
    .with_warnings(std::mem::take(&mut ctx.warnings)))
}

/// `reset --yes`: back up, then empty the store in one transaction so
/// concurrent readers never see a missing file.
pub fn reset(ctx: &mut Ctx, yes: bool) -> Result<Outcome, SillokError> {
    if !yes {
        return Err(SillokError::invalid(
            "confirmation_required",
            "reset deletes every record; pass --yes",
        ));
    }
    let mut store = match ctx.open_or_create() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let backup = with_suffix(
        store.path(),
        &format!(".reset-{}.bak.db", ctx.now.as_millis()),
    );
    if let Err(error) = store
        .conn
        .execute("VACUUM INTO ?1", [backup.display().to_string()])
    {
        return Err(error.into());
    }
    let info = StoreInfo {
        archive_id: ArchiveId::new_v7(),
        created_at: ctx.now,
    };
    let tx = match store
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
    {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    if let Err(error) =
        tx.execute_batch("DELETE FROM record; DELETE FROM event; DELETE FROM work_context;")
    {
        return Err(error.into());
    }
    if let Err(error) = write_info(&tx, info) {
        return Err(error);
    }
    if let Err(error) = tx.commit() {
        return Err(error.into());
    }
    Ok(Outcome::write(
        "reset",
        vec![info.archive_id.to_string()],
        json!({ "archive_id": info.archive_id, "backup": backup.display().to_string() }),
    )
    .with_warnings(std::mem::take(&mut ctx.warnings)))
}

/// `export`: JSON lines on stdout, streamed.
pub fn export(ctx: &mut Ctx, args: ExportArgs) -> Result<Outcome, SillokError> {
    let (from_raw, to_raw) = match args.format {
        Some(ExportFormat::Json(range)) => (range.from, range.to),
        None => (args.from, args.to),
    };
    let (from, to) = match (
        ctx.instant(from_raw.as_deref()),
        ctx.instant(to_raw.as_deref()),
    ) {
        (Ok(from), Ok(to)) => (from, to),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    let store = match ctx.open() {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(Outcome::streamed("export", json!({ "lines": 0 }))),
        Err(error) => return Err(error),
    };
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut sink = |line: &str| -> Result<(), SillokError> {
        match writeln!(out, "{line}") {
            Ok(()) => Ok(()),
            Err(error) => Err(error.into()),
        }
    };
    let written = match args.events {
        true => events::stream(&store.conn, from, to, &mut sink),
        false => {
            let filter = RecordFilter {
                from,
                to,
                limit: usize::MAX,
                ..RecordFilter::default()
            };
            match store.query(&filter) {
                Ok(records) => {
                    let mut count = 0usize;
                    let mut failure = None;
                    for record in &records {
                        match sink(&views::record(record, true).to_string()) {
                            Ok(()) => count += 1,
                            Err(error) => {
                                failure = Some(error);
                                break;
                            }
                        }
                    }
                    match failure {
                        Some(error) => Err(error),
                        None => Ok(count),
                    }
                }
                Err(error) => Err(error),
            }
        }
    };
    let flushed = out.flush();
    match (written, flushed) {
        (Ok(count), Ok(())) => Ok(Outcome::streamed("export", json!({ "lines": count }))
            .with_warnings(std::mem::take(&mut ctx.warnings))),
        // A reader that closed the pipe (`| head`) got what it wanted.
        (Err(SillokError::Io(error)), _) | (_, Err(error))
            if error.kind() == ErrorKind::BrokenPipe =>
        {
            Ok(Outcome::streamed("export", json!({ "truncated": true })))
        }
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
    }
}

/// `guide`.
pub fn guide() -> Outcome {
    Outcome::text("guide", GUIDE.trim_end().to_string())
}
