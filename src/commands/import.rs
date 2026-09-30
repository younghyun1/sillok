//! `import` and the 0.10 `migrate` alias.
//!
//! Sources are detected from their bytes, not their names:
//! - SQLite files: a 0.9/0.10 store (converted) or a 1.x store (copied);
//! - zstd files: a 0.x `.slk.zst` archive or sync artifact (converted);
//! - directories: a 1.x sync layout (`manifest.json` + `events/`);
//! - anything else: JSON lines from `sillok export --events`.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::cli::args::admin::MigrateArgs;
use crate::cli::output::outcome::Outcome;
use crate::commands::ctx::Ctx;
use crate::domain::event::envelope::Event;
use crate::domain::id::EventId;
use crate::domain::merge;
use crate::error::SillokError;
use crate::legacy::{codec, convert, v2_store};
use crate::storage::sqlite::{events, open};
use crate::sync::layout;

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Events read from a source.
struct Source {
    format: &'static str,
    events: Vec<Event>,
    dropped: usize,
}

/// `import <path>`.
pub fn import(ctx: &mut Ctx, path: &Path, dry_run: bool) -> Result<Outcome, SillokError> {
    let source = match read_source(path) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let read = source.events.len();
    let mut store = match ctx.open_or_create() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let local = match events::lines(&store.conn) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let local_raw: HashMap<EventId, &str> = local
        .iter()
        .map(|line| (line.event_id, line.raw.as_str()))
        .collect();
    let plan = merge::plan(&local_raw, source.events);
    let (added, replaced, records) = match dry_run {
        true => (plan.additions.len(), plan.replacements.len(), None),
        false => match store.merge(&plan.additions, &plan.replacements) {
            Ok(summary) => (
                summary.added,
                summary.replaced,
                summary.rebuild.map(|rebuild| rebuild.records),
            ),
            Err(error) => return Err(error),
        },
    };
    let human = format!(
        "{}{}: read {read}, added {added}, replaced {replaced}, dropped {} legacy markers",
        if dry_run { "dry run " } else { "" },
        source.format,
        source.dropped
    );
    Ok(Outcome::read(
        "import",
        json!({
            "source": path.display().to_string(),
            "format": source.format,
            "dry_run": dry_run,
            "events_read": read,
            "added": added,
            "replaced": replaced,
            "conflicts": plan.conflicts.len(),
            "dropped": source.dropped,
            "records": records,
        }),
    )
    .with_human(human)
    .with_warnings(std::mem::take(&mut ctx.warnings)))
}

/// `[--store <legacy>] migrate [--target <db>] --yes` from 0.10.
///
/// As in 0.10, the source defaults to the v1 archive `sillok.slk.zst` beside
/// the store, and the target to `sillok.db` beside the source.
pub fn migrate(ctx: &mut Ctx, args: MigrateArgs) -> Result<Outcome, SillokError> {
    if !args.yes && !args.dry_run {
        return Err(SillokError::invalid(
            "confirmation_required",
            "migrate needs --yes or --dry-run",
        ));
    }
    let named_legacy = ctx
        .store_path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".slk.zst"));
    let source = match named_legacy {
        true => ctx.store_path.clone(),
        false => ctx.store_path.with_file_name("sillok.slk.zst"),
    };
    ctx.store_path = match args.target {
        Some(target) => target,
        None => match source.parent() {
            Some(parent) => parent.join("sillok.db"),
            None => PathBuf::from("sillok.db"),
        },
    };
    import(ctx, &source, args.dry_run)
}

fn read_source(path: &Path) -> Result<Source, SillokError> {
    if path.is_dir() {
        return match layout::read(path) {
            Ok(found) => match parse_all(found.lines.into_iter().map(|line| line.raw)) {
                Ok(events) => Ok(Source {
                    format: "sync_directory",
                    events,
                    dropped: 0,
                }),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
    }
    let mut head = [0u8; 16];
    let read = match std::fs::File::open(path) {
        Ok(mut file) => match file.read(&mut head) {
            Ok(count) => count,
            Err(error) => return Err(error.into()),
        },
        Err(error) => return Err(error.into()),
    };
    if read == 16 && &head == SQLITE_MAGIC {
        return read_sqlite(path);
    }
    if codec::is_zstd(&head[..read]) {
        let bytes = match std::fs::read(path) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        return match codec::decode_archive(&bytes) {
            Ok(archive) => match convert::convert(&archive) {
                Ok(conversion) => Ok(Source {
                    format: "legacy_archive",
                    events: conversion.events,
                    dropped: conversion.dropped,
                }),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
    }
    let text = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    match parse_all(
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string),
    ) {
        Ok(events) => Ok(Source {
            format: "events_jsonl",
            events,
            dropped: 0,
        }),
        Err(error) => Err(error),
    }
}

fn read_sqlite(path: &Path) -> Result<Source, SillokError> {
    let conn = match open::connect(path, false) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    match v2_store::is_v2_store(&conn) {
        Ok(true) => match v2_store::read_v2_store(&conn) {
            Ok(archive) => match convert::convert(&archive) {
                Ok(conversion) => Ok(Source {
                    format: "v2_store",
                    events: conversion.events,
                    dropped: conversion.dropped,
                }),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        },
        Ok(false) => match events::load_all(&conn) {
            Ok(found) => Ok(Source {
                format: "store",
                events: found,
                dropped: 0,
            }),
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    }
}

fn parse_all(lines: impl Iterator<Item = String>) -> Result<Vec<Event>, SillokError> {
    let mut out = Vec::new();
    for line in lines {
        match Event::parse(line) {
            Ok(event) => out.push(event),
            Err(error) => return Err(error),
        }
    }
    Ok(out)
}
