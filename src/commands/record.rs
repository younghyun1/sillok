//! Commands that write one event: note, objective add/complete, amend,
//! move, retract, restore.

use serde_json::json;

use crate::cli::args::record::{
    AmendArgs, MoveArgs, NoteArgs, ObjectiveAddArgs, ObjectiveCompleteArgs, RetractArgs, StatusArg,
};
use crate::cli::human::records as human;
use crate::cli::output::outcome::Outcome;
use crate::cli::output::views;
use crate::commands::ctx::{Ctx, optional_id};
use crate::domain::event::kind::EventKind;
use crate::domain::id::RecordId;
use crate::domain::record::{Record, RecordKind, RecordStatus};
use crate::domain::text;
use crate::error::SillokError;

/// `note`.
pub fn note(ctx: &mut Ctx, args: NoteArgs) -> Result<Outcome, SillokError> {
    let status = match writable_status(args.status) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let kind = match (
        text::entry(args.text),
        text::optional_detail(args.purpose, "purpose"),
        text::tags(args.tags),
        optional_id(args.parent.as_deref()),
    ) {
        (Ok(text), Ok(purpose), Ok(tags), Ok(parent_id)) => EventKind::TaskRecorded {
            record_id: RecordId::new_v7(),
            parent_id,
            text,
            purpose,
            tags,
            status,
        },
        (Err(error), ..) | (_, Err(error), ..) | (_, _, Err(error), _) | (.., Err(error)) => {
            return Err(error);
        }
    };
    append(ctx, "note", kind)
}

/// `objective add`.
pub fn objective_add(ctx: &mut Ctx, args: ObjectiveAddArgs) -> Result<Outcome, SillokError> {
    let status = match writable_status(args.status) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let kind = match (
        text::entry(args.text),
        text::tags(args.tags),
        optional_id(args.parent.as_deref()),
    ) {
        (Ok(text), Ok(tags), Ok(parent_id)) => EventKind::ObjectiveAdded {
            record_id: RecordId::new_v7(),
            parent_id,
            text,
            tags,
            status,
        },
        (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => return Err(error),
    };
    append(ctx, "objective", kind)
}

/// `objective complete`.
pub fn objective_complete(
    ctx: &mut Ctx,
    args: ObjectiveCompleteArgs,
) -> Result<Outcome, SillokError> {
    let (record_id, note) = match (
        RecordId::parse(&args.id),
        text::optional_detail(args.note, "note"),
    ) {
        (Ok(id), Ok(note)) => (id, note),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    // Kind never changes after creation, so checking before the write is race-free.
    match require(ctx, record_id) {
        Ok(record) if record.kind == RecordKind::Objective => {}
        Ok(_) => {
            return Err(SillokError::operation(
                "invalid_record_kind",
                format!("record `{record_id}` is a task; use `amend --status completed`"),
            ));
        }
        Err(error) => return Err(error),
    }
    append(
        ctx,
        "objective",
        EventKind::RecordAmended {
            record_id,
            text: None,
            status: Some(RecordStatus::Completed),
            purpose: None,
            clear_purpose: false,
            tags: None,
            note,
        },
    )
}

/// `amend`.
pub fn amend(ctx: &mut Ctx, args: AmendArgs) -> Result<Outcome, SillokError> {
    let record_id = match RecordId::parse(&args.id) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let status = match args.status {
        Some(value) => match writable_status(value) {
            Ok(status) => Some(status),
            Err(error) => return Err(error),
        },
        None => None,
    };
    let (text, purpose, note, tags) = match (
        optional(args.text, text::entry),
        text::optional_detail(args.purpose, "purpose"),
        text::optional_detail(args.note, "note"),
        text::tags(args.tags),
    ) {
        (Ok(text), Ok(purpose), Ok(note), Ok(tags)) => (text, purpose, note, tags),
        (Err(error), ..) | (_, Err(error), ..) | (_, _, Err(error), _) | (.., Err(error)) => {
            return Err(error);
        }
    };
    let tags = match (args.clear_tags, tags.is_empty()) {
        (true, _) => Some(Vec::new()),
        (false, true) => None,
        (false, false) => Some(tags),
    };
    if text.is_none()
        && status.is_none()
        && purpose.is_none()
        && !args.clear_purpose
        && tags.is_none()
        && note.is_none()
    {
        return Err(SillokError::invalid(
            "empty_amendment",
            "amend needs at least one field to change",
        ));
    }
    append(
        ctx,
        "amend",
        EventKind::RecordAmended {
            record_id,
            text,
            status,
            purpose,
            clear_purpose: args.clear_purpose,
            tags,
            note,
        },
    )
}

/// `move`.
pub fn move_record(ctx: &mut Ctx, args: MoveArgs) -> Result<Outcome, SillokError> {
    let (record_id, parent_id) = match (
        RecordId::parse(&args.id),
        optional_id(args.parent.as_deref()),
    ) {
        (Ok(id), Ok(parent)) => (id, parent),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    append(
        ctx,
        "move",
        EventKind::RecordMoved {
            record_id,
            parent_id,
        },
    )
}

/// `retract`.
pub fn retract(ctx: &mut Ctx, args: RetractArgs) -> Result<Outcome, SillokError> {
    let (record_id, reason) = match (
        RecordId::parse(&args.id),
        text::detail(args.reason, "reason"),
    ) {
        (Ok(id), Ok(reason)) => (id, reason),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    append(
        ctx,
        "retract",
        EventKind::RecordRetracted { record_id, reason },
    )
}

/// `restore`.
pub fn restore(ctx: &mut Ctx, id: &str) -> Result<Outcome, SillokError> {
    match RecordId::parse(id) {
        Ok(record_id) => append(ctx, "restore", EventKind::RecordRestored { record_id }),
        Err(error) => Err(error),
    }
}

fn append(ctx: &mut Ctx, command: &'static str, kind: EventKind) -> Result<Outcome, SillokError> {
    let mut store = match ctx.open_or_create() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let stamp = ctx.stamp();
    let record = match store.append(stamp, kind) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    Ok(written(ctx, command, &record))
}

fn written(ctx: &mut Ctx, command: &'static str, record: &Record) -> Outcome {
    Outcome::write(
        command,
        vec![record.id.to_string()],
        json!({ "id": record.id, "record": views::record(record, ctx.full) }),
    )
    .with_human(human::line(record, &ctx.zone))
    .with_warnings(std::mem::take(&mut ctx.warnings))
}

fn require(ctx: &mut Ctx, id: RecordId) -> Result<Record, SillokError> {
    let store = match ctx.open() {
        Ok(Some(value)) => value,
        Ok(None) => return Err(SillokError::RecordNotFound(id.to_string())),
        Err(error) => return Err(error),
    };
    match store.record(id) {
        Ok(Some(record)) => Ok(record),
        Ok(None) => Err(SillokError::RecordNotFound(id.to_string())),
        Err(error) => Err(error),
    }
}

/// Writes may not set `retracted`; that goes through `retract` so a reason is kept.
fn writable_status(status: StatusArg) -> Result<RecordStatus, SillokError> {
    match status {
        StatusArg::Retracted => Err(SillokError::invalid(
            "invalid_status",
            "status `retracted` is set with `sillok retract <id> --reason ...`",
        )),
        other => Ok(other.status()),
    }
}

fn optional(
    value: Option<String>,
    check: fn(String) -> Result<String, SillokError>,
) -> Result<Option<String>, SillokError> {
    match value {
        Some(raw) => match check(raw) {
            Ok(clean) => Ok(Some(clean)),
            Err(error) => Err(error),
        },
        None => Ok(None),
    }
}
