//! `note --stdin`: several tasks in one command and one transaction.
//!
//! An agent that finished three things pays three tool calls to say so. One
//! JSON object per line records them together; flags on the command line
//! are the defaults for lines that leave a field out.

use std::io::Read;

use clap::ValueEnum;
use serde::Deserialize;
use serde_json::json;

use crate::cli::args::record::{NoteArgs, StatusArg};
use crate::cli::output::outcome::Outcome;
use crate::cli::output::views;
use crate::commands::ctx::Ctx;
use crate::commands::record::writable_status;
use crate::domain::event::kind::EventKind;
use crate::domain::id::RecordId;
use crate::domain::text;
use crate::error::SillokError;

/// Most tasks accepted in one batch.
pub const MAX_LINES: usize = 500;

/// Most input read from stdin.
pub const MAX_BYTES: u64 = 1 << 20;

/// One input line. Unknown fields are rejected so a typo is not dropped.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    text: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    purpose: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    status: Option<String>,
}

/// `note --stdin`.
pub fn notes(ctx: &mut Ctx, args: NoteArgs) -> Result<Outcome, SillokError> {
    let mut input = String::new();
    if let Err(error) = std::io::stdin()
        .take(MAX_BYTES + 1)
        .read_to_string(&mut input)
    {
        return Err(error.into());
    }
    if input.len() as u64 > MAX_BYTES {
        return Err(SillokError::invalid(
            "batch_too_large",
            format!("stdin holds more than {MAX_BYTES} bytes"),
        ));
    }
    let mut kinds = Vec::new();
    for (index, raw) in input.lines().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        if kinds.len() == MAX_LINES {
            return Err(SillokError::invalid(
                "batch_too_large",
                format!("a batch holds at most {MAX_LINES} tasks"),
            ));
        }
        match task(ctx, &args, raw) {
            Ok(kind) => kinds.push(kind),
            Err(error) => {
                return Err(SillokError::invalid(
                    "invalid_batch_line",
                    format!("line {}: {error}", index + 1),
                ));
            }
        }
    }
    if kinds.is_empty() {
        return Err(SillokError::invalid(
            "empty_batch",
            "stdin held no tasks; send one JSON object per line",
        ));
    }
    let mut store = match ctx.open_or_create() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let stamp = ctx.stamp();
    let records = match store.append_all(stamp, kinds) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let ids: Vec<String> = records.iter().map(|record| record.id.to_string()).collect();
    Ok(Outcome::write(
        "note",
        ids.clone(),
        json!({ "ids": ids, "records": views::records(&records, ctx.full) }),
    )
    .with_warnings(std::mem::take(&mut ctx.warnings)))
}

/// Builds the event for one line, falling back to the command's flags.
fn task(ctx: &mut Ctx, args: &NoteArgs, raw: &str) -> Result<EventKind, SillokError> {
    let line: Line = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(error) => {
            return Err(SillokError::invalid(
                "invalid_batch_line",
                error.to_string(),
            ));
        }
    };
    let status = match &line.status {
        Some(name) => match StatusArg::from_str(name, true) {
            Ok(value) => value,
            Err(_) => {
                return Err(SillokError::invalid(
                    "invalid_status",
                    format!("unknown status `{name}`"),
                ));
            }
        },
        None => args.status,
    };
    let parent = match &line.parent {
        Some(value) => Some(value.as_str()),
        None => args.parent.as_deref(),
    };
    let tags = match line.tags {
        Some(value) => value,
        None => args.tags.clone(),
    };
    let purpose = match line.purpose {
        Some(value) => Some(value),
        None => args.purpose.clone(),
    };
    match (
        writable_status(status),
        text::entry(line.text),
        text::optional_detail(purpose, "purpose"),
        text::tags(tags),
        ctx.optional_record_id(parent),
    ) {
        (Ok(status), Ok(text), Ok(purpose), Ok(tags), Ok(parent_id)) => {
            Ok(EventKind::TaskRecorded {
                record_id: RecordId::new_v7(),
                parent_id,
                text,
                purpose,
                tags,
                status,
            })
        }
        (Err(error), ..)
        | (_, Err(error), ..)
        | (_, _, Err(error), ..)
        | (_, _, _, Err(error), _)
        | (.., Err(error)) => Err(error),
    }
}
