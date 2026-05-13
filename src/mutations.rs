use serde_json::json;

use crate::cli::args::{
    AmendArgs, NoteArgs, ObjectiveAddArgs, ObjectiveCompleteArgs, RetractArgs, TruncateArgs,
};
use crate::cli::human;
use crate::cli::output::CommandOutcome;
use crate::domain::event::RecordKind;
use crate::domain::id::ChronicleId;
use crate::error::SillokError;
use crate::mutation_helpers::parse_optional_id;
use crate::operation::{OperationContext, clean_entry, clean_purpose, clean_reason, clean_tags};
use crate::storage::sql::store::{
    AmendInput, CompleteObjectiveInput, ObjectiveInput, RetractInput, TaskInput,
};

/// Handles `init`.
pub async fn init(ctx: OperationContext) -> Result<CommandOutcome, SillokError> {
    let store = ctx.store.require_sql_mutation()?;
    let (info, created) = store
        .init(ctx.recorded_at, ctx.actor(), ctx.context())
        .await?;
    Ok(CommandOutcome::new(
        "init",
        json!({
            "created": created,
            "store": ctx.store.path().display().to_string(),
            "archive_id": info.archive_id,
            "created_at": info.created_at,
            "store_datashape_version": crate::storage::sql::schema::STORE_DATASHAPE_VERSION,
        }),
    )
    .with_ids(json!({ "archive_id": info.archive_id }))
    .with_warnings(ctx.warnings)
    .with_human(human::store_init(
        info.archive_id,
        info.created_at,
        created,
        ctx.store.path(),
    )))
}

/// Handles `note`.
pub async fn note(ctx: OperationContext, args: NoteArgs) -> Result<CommandOutcome, SillokError> {
    let text = clean_entry(args.text)?;
    let purpose = clean_purpose(args.purpose)?;
    let tags = clean_tags(args.tags)?;
    let parent = parse_optional_id(args.parent)?;
    let day_key = ctx.zone.day_key(ctx.event_at)?;
    let store = ctx.store.require_sql_mutation()?;
    let warnings = ctx.warnings.clone();
    let record = store
        .record_task(TaskInput {
            recorded_at: ctx.recorded_at,
            event_at: ctx.event_at,
            actor: ctx.actor(),
            context: ctx.context(),
            day_key,
            parent,
            text,
            purpose,
            tags,
            status: args.status,
        })
        .await?;
    let human = human::record_action("Recorded task", &record);
    Ok(CommandOutcome::new("note", json!({ "record": record }))
        .with_ids(json!({ "task_id": record.record_id, "day_id": record.day_id }))
        .with_warnings(warnings)
        .with_human(human))
}

/// Handles `objective add`.
pub async fn objective_add(
    ctx: OperationContext,
    args: ObjectiveAddArgs,
) -> Result<CommandOutcome, SillokError> {
    let text = clean_entry(args.text)?;
    let tags = clean_tags(args.tags)?;
    let store = ctx.store.require_sql_mutation()?;
    let warnings = ctx.warnings.clone();
    let day_key = ctx.zone.day_key(ctx.event_at)?;
    let record = store
        .add_objective(ObjectiveInput {
            recorded_at: ctx.recorded_at,
            event_at: ctx.event_at,
            actor: ctx.actor(),
            context: ctx.context(),
            day_key,
            text,
            tags,
        })
        .await?;
    let human = human::record_action("Added objective", &record);
    Ok(
        CommandOutcome::new("objective", json!({ "record": record }))
            .with_ids(json!({ "objective_id": record.record_id, "day_id": record.day_id }))
            .with_warnings(warnings)
            .with_human(human),
    )
}

/// Handles `objective complete`.
pub async fn objective_complete(
    ctx: OperationContext,
    args: ObjectiveCompleteArgs,
) -> Result<CommandOutcome, SillokError> {
    let objective_id = ChronicleId::parse(&args.id)?;
    let note = clean_purpose(args.note)?;
    let store = ctx.store.require_sql_mutation()?;
    let warnings = ctx.warnings.clone();
    let record = store
        .complete_objective(CompleteObjectiveInput {
            recorded_at: ctx.recorded_at,
            event_at: ctx.event_at,
            actor: ctx.actor(),
            context: ctx.context(),
            objective_id,
            note,
        })
        .await?;
    if record.kind != RecordKind::Objective {
        return Err(SillokError::new(
            "invalid_record_kind",
            format!("record `{objective_id}` is not an objective"),
        ));
    }
    let human = human::record_action("Completed objective", &record);
    Ok(
        CommandOutcome::new("objective", json!({ "record": record }))
            .with_ids(json!({ "objective_id": objective_id }))
            .with_warnings(warnings)
            .with_human(human),
    )
}

/// Handles `amend`.
pub async fn amend(ctx: OperationContext, args: AmendArgs) -> Result<CommandOutcome, SillokError> {
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
    let store = ctx.store.require_sql_mutation()?;
    let warnings = ctx.warnings.clone();
    let record = store
        .amend_record(AmendInput {
            recorded_at: ctx.recorded_at,
            event_at: ctx.event_at,
            actor: ctx.actor(),
            context: ctx.context(),
            record_id,
            text,
            status: args.status,
            purpose,
            tags: tags_update,
        })
        .await?;
    let human = human::record_action("Amended record", &record);
    Ok(CommandOutcome::new("amend", json!({ "record": record }))
        .with_ids(json!({ "record_id": record_id }))
        .with_warnings(warnings)
        .with_human(human))
}

/// Handles `retract`.
pub async fn retract(
    ctx: OperationContext,
    args: RetractArgs,
) -> Result<CommandOutcome, SillokError> {
    let record_id = ChronicleId::parse(&args.id)?;
    let reason = clean_reason(args.reason)?;
    let store = ctx.store.require_sql_mutation()?;
    let warnings = ctx.warnings.clone();
    let record = store
        .retract_record(RetractInput {
            recorded_at: ctx.recorded_at,
            event_at: ctx.event_at,
            actor: ctx.actor(),
            context: ctx.context(),
            record_id,
            reason,
        })
        .await?;
    let human = human::record_action("Retracted record", &record);
    Ok(CommandOutcome::new("retract", json!({ "record": record }))
        .with_ids(json!({ "record_id": record_id }))
        .with_warnings(warnings)
        .with_human(human))
}

/// Handles `truncate`.
pub async fn truncate(
    ctx: OperationContext,
    args: TruncateArgs,
) -> Result<CommandOutcome, SillokError> {
    if !args.yes {
        return Err(SillokError::new(
            "confirmation_required",
            "truncate requires --yes",
        ));
    }
    let store = ctx.store.require_sql_mutation()?;
    let (info, backup) = store
        .truncate(ctx.recorded_at, ctx.actor(), ctx.context())
        .await?;
    let human = human::store_truncate(
        info.archive_id,
        info.created_at,
        backup.as_deref(),
        ctx.store.path(),
    );
    Ok(CommandOutcome::new(
        "truncate",
        json!({
            "archive_id": info.archive_id,
            "created_at": info.created_at,
            "backup": backup.as_ref().map(|path| path.display().to_string()),
            "store": ctx.store.path().display().to_string(),
            "store_datashape_version": crate::storage::sql::schema::STORE_DATASHAPE_VERSION,
        }),
    )
    .with_ids(json!({ "archive_id": info.archive_id }))
    .with_warnings(ctx.warnings)
    .with_human(human))
}
