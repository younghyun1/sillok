use serde_json::json;

use crate::cli::args::{
    AmendArgs, NoteArgs, ObjectiveAddArgs, ObjectiveCompleteArgs, RetractArgs, TruncateArgs,
};
use crate::cli::human;
use crate::cli::output::CommandOutcome;
use crate::domain::event::{ChronicleEvent, EventKind, RecordKind};
use crate::domain::id::ChronicleId;
use crate::domain::view::ChronicleView;
use crate::error::SillokError;
use crate::mutation_helpers::{
    day_for_key, day_opened_event, parent_target, parse_optional_id, require_active_record,
    require_output_record,
};
use crate::operation::{OperationContext, clean_entry, clean_purpose, clean_reason, clean_tags};

/// Handles `init`.
pub fn init(ctx: OperationContext) -> Result<CommandOutcome, SillokError> {
    let (archive, created) = ctx
        .store
        .init(ctx.recorded_at, ctx.actor(), ctx.context())?;
    Ok(CommandOutcome::new(
        "init",
        json!({
            "created": created,
            "store": ctx.store.path().display().to_string(),
            "archive_id": archive.archive_id,
            "created_at": archive.created_at,
        }),
    )
    .with_ids(json!({ "archive_id": archive.archive_id }))
    .with_warnings(ctx.warnings)
    .with_human(human::init(&archive, created, ctx.store.path())))
}

/// Handles `note`.
pub fn note(ctx: OperationContext, args: NoteArgs) -> Result<CommandOutcome, SillokError> {
    let text = clean_entry(args.text)?;
    let purpose = clean_purpose(args.purpose)?;
    let tags = clean_tags(args.tags)?;
    let parent = parse_optional_id(args.parent)?;
    let store = ctx.store.clone();
    let warnings = ctx.warnings.clone();
    let outcome = store.mutate(ctx.recorded_at, ctx.actor(), ctx.context(), |archive| {
        let view = ChronicleView::build(archive)?;
        let (day_id, parent_id, day_to_open) = parent_target(&ctx, &view, parent)?;
        drop(view);
        if let Some((new_day_id, day_key)) = day_to_open {
            archive.push(day_opened_event(&ctx, new_day_id, day_key));
        }
        let task_id = ChronicleId::new_v7();
        archive.push(ChronicleEvent::new(
            ctx.event_at,
            ctx.recorded_at,
            ctx.actor(),
            ctx.context(),
            EventKind::TaskRecorded {
                task_id,
                day_id,
                parent_id,
                text: text.clone(),
                purpose: purpose.clone(),
                tags: tags.clone(),
                status: args.status,
            },
        ));
        let view = ChronicleView::build(archive)?;
        let record = require_output_record(&view, task_id)?;
        let human = human::record_action("Recorded task", &record);
        Ok(CommandOutcome::new("note", json!({ "record": record }))
            .with_ids(json!({ "task_id": task_id, "day_id": day_id }))
            .with_human(human))
    })?;
    Ok(outcome.with_warnings(warnings))
}

/// Handles `objective add`.
pub fn objective_add(
    ctx: OperationContext,
    args: ObjectiveAddArgs,
) -> Result<CommandOutcome, SillokError> {
    let text = clean_entry(args.text)?;
    let tags = clean_tags(args.tags)?;
    let store = ctx.store.clone();
    let warnings = ctx.warnings.clone();
    let day_key = ctx.zone.day_key(ctx.event_at)?;
    let outcome = store.mutate(ctx.recorded_at, ctx.actor(), ctx.context(), |archive| {
        let view = ChronicleView::build(archive)?;
        let (day_id, day_to_open) = day_for_key(&view, day_key.clone());
        drop(view);
        if let Some((new_day_id, key)) = day_to_open {
            archive.push(day_opened_event(&ctx, new_day_id, key));
        }
        let objective_id = ChronicleId::new_v7();
        archive.push(ChronicleEvent::new(
            ctx.event_at,
            ctx.recorded_at,
            ctx.actor(),
            ctx.context(),
            EventKind::ObjectiveAdded {
                objective_id,
                day_id,
                text: text.clone(),
                tags: tags.clone(),
            },
        ));
        let view = ChronicleView::build(archive)?;
        let record = require_output_record(&view, objective_id)?;
        let human = human::record_action("Added objective", &record);
        Ok(
            CommandOutcome::new("objective", json!({ "record": record }))
                .with_ids(json!({ "objective_id": objective_id, "day_id": day_id }))
                .with_human(human),
        )
    })?;
    Ok(outcome.with_warnings(warnings))
}

/// Handles `objective complete`.
pub fn objective_complete(
    ctx: OperationContext,
    args: ObjectiveCompleteArgs,
) -> Result<CommandOutcome, SillokError> {
    let objective_id = ChronicleId::parse(&args.id)?;
    let note = clean_purpose(args.note)?;
    let store = ctx.store.clone();
    let warnings = ctx.warnings.clone();
    let outcome = store.mutate(ctx.recorded_at, ctx.actor(), ctx.context(), |archive| {
        let view = ChronicleView::build(archive)?;
        let record = require_active_record(&view, objective_id)?;
        if record.kind != RecordKind::Objective {
            return Err(SillokError::new(
                "invalid_record_kind",
                format!("record `{objective_id}` is not an objective"),
            ));
        }
        drop(view);
        archive.push(ChronicleEvent::new(
            ctx.event_at,
            ctx.recorded_at,
            ctx.actor(),
            ctx.context(),
            EventKind::ObjectiveCompleted { objective_id, note },
        ));
        let view = ChronicleView::build(archive)?;
        let record = require_output_record(&view, objective_id)?;
        let human = human::record_action("Completed objective", &record);
        Ok(
            CommandOutcome::new("objective", json!({ "record": record }))
                .with_ids(json!({ "objective_id": objective_id }))
                .with_human(human),
        )
    })?;
    Ok(outcome.with_warnings(warnings))
}

/// Handles `amend`.
pub fn amend(ctx: OperationContext, args: AmendArgs) -> Result<CommandOutcome, SillokError> {
    let record_id = ChronicleId::parse(&args.id)?;
    let text = match args.text {
        Some(value) => Some(clean_entry(value)?),
        None => None,
    };
    let purpose = clean_purpose(args.purpose)?;
    let tags = clean_tags(args.tags)?;
    let tags_update = if tags.is_empty() { None } else { Some(tags) };
    if text.is_none() && args.status.is_none() && purpose.is_none() && tags_update.is_none() {
        return Err(SillokError::new(
            "empty_amendment",
            "amend requires at least one changed field",
        ));
    }
    let store = ctx.store.clone();
    let warnings = ctx.warnings.clone();
    let outcome = store.mutate(ctx.recorded_at, ctx.actor(), ctx.context(), |archive| {
        let view = ChronicleView::build(archive)?;
        require_active_record(&view, record_id)?;
        drop(view);
        archive.push(ChronicleEvent::new(
            ctx.event_at,
            ctx.recorded_at,
            ctx.actor(),
            ctx.context(),
            EventKind::TaskAmended {
                record_id,
                text: text.clone(),
                status: args.status,
                purpose: purpose.clone(),
                tags: tags_update.clone(),
            },
        ));
        let view = ChronicleView::build(archive)?;
        let record = require_output_record(&view, record_id)?;
        let human = human::record_action("Amended record", &record);
        Ok(CommandOutcome::new("amend", json!({ "record": record }))
            .with_ids(json!({ "record_id": record_id }))
            .with_human(human))
    })?;
    Ok(outcome.with_warnings(warnings))
}

/// Handles `retract`.
pub fn retract(ctx: OperationContext, args: RetractArgs) -> Result<CommandOutcome, SillokError> {
    let record_id = ChronicleId::parse(&args.id)?;
    let reason = clean_reason(args.reason)?;
    let store = ctx.store.clone();
    let warnings = ctx.warnings.clone();
    let outcome = store.mutate(ctx.recorded_at, ctx.actor(), ctx.context(), |archive| {
        let view = ChronicleView::build(archive)?;
        let record = require_active_record(&view, record_id)?;
        if record.kind == RecordKind::Day {
            return Err(SillokError::new(
                "invalid_operation",
                "day records cannot be retracted; use truncate for a full reset",
            ));
        }
        drop(view);
        archive.push(ChronicleEvent::new(
            ctx.event_at,
            ctx.recorded_at,
            ctx.actor(),
            ctx.context(),
            EventKind::TaskRetracted { record_id, reason },
        ));
        let view = ChronicleView::build(archive)?;
        let record = require_output_record(&view, record_id)?;
        let human = human::record_action("Retracted record", &record);
        Ok(CommandOutcome::new("retract", json!({ "record": record }))
            .with_ids(json!({ "record_id": record_id }))
            .with_human(human))
    })?;
    Ok(outcome.with_warnings(warnings))
}

/// Handles `truncate`.
pub fn truncate(ctx: OperationContext, args: TruncateArgs) -> Result<CommandOutcome, SillokError> {
    if !args.yes {
        return Err(SillokError::new(
            "confirmation_required",
            "truncate requires --yes",
        ));
    }
    let backup = ctx
        .store
        .truncate(ctx.recorded_at, ctx.actor(), ctx.context())?;
    let archive = ctx
        .store
        .read_or_new(ctx.recorded_at, ctx.actor(), ctx.context())?;
    let human = human::truncate(&archive, backup.as_deref(), ctx.store.path());
    Ok(CommandOutcome::new(
        "truncate",
        json!({
            "archive_id": archive.archive_id,
            "created_at": archive.created_at,
            "backup": backup.as_ref().map(|path| path.display().to_string()),
            "store": ctx.store.path().display().to_string(),
        }),
    )
    .with_ids(json!({ "archive_id": archive.archive_id }))
    .with_warnings(ctx.warnings)
    .with_human(human))
}
