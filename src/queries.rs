use serde_json::json;

use crate::cli::args::{DayArgs, ExportJsonArgs, IdArgs, QueryArgs, TreeArgs};
use crate::cli::human;
use crate::cli::output::CommandOutcome;
use crate::domain::event::RecordKind;
use crate::domain::id::ChronicleId;
use crate::domain::time::Timestamp;
use crate::domain::view::ChronicleView;
use crate::error::SillokError;
use crate::operation::OperationContext;

/// Handles `show`.
pub fn show(ctx: OperationContext, args: IdArgs) -> Result<CommandOutcome, SillokError> {
    let record_id = ChronicleId::parse(&args.id)?;
    let archive = ctx
        .store
        .read_or_new(ctx.recorded_at, ctx.actor(), ctx.context())?;
    let view = ChronicleView::build(&archive)?;
    let record = match view.record(&record_id) {
        Some(value) => value.clone(),
        None => {
            return Err(SillokError::new(
                "record_not_found",
                format!("record `{record_id}` does not exist"),
            ));
        }
    };
    let events = view.events_for_record(record_id);
    Ok(
        CommandOutcome::new("show", json!({ "record": record, "events": events }))
            .with_ids(json!({ "record_id": record_id }))
            .with_warnings(ctx.warnings)
            .with_human(format!("{} {}", record.record_id, record.text)),
    )
}

/// Handles `day`.
pub fn day(ctx: OperationContext, args: DayArgs) -> Result<CommandOutcome, SillokError> {
    let day_key = match args.date {
        Some(date) => ctx.zone.parse_date(&date)?,
        None => ctx.zone.day_key(ctx.event_at)?,
    };
    let archive = ctx
        .store
        .read_or_new(ctx.recorded_at, ctx.actor(), ctx.context())?;
    let view = ChronicleView::build(&archive)?;
    match view.day_id(&day_key) {
        Some(day_id) => {
            let tree = view.tree(day_id)?;
            let records = view.records_for_day(day_id);
            let objectives = records
                .iter()
                .filter(|record| record.kind == RecordKind::Objective)
                .cloned()
                .collect::<Vec<_>>();
            let human = human::day(&day_key, records.len(), Some(&tree));
            Ok(CommandOutcome::new(
                "day",
                json!({
                    "day_key": day_key,
                    "day_id": day_id,
                    "tree": tree,
                    "objectives": objectives,
                    "records": records,
                }),
            )
            .with_ids(json!({ "day_id": day_id }))
            .with_warnings(ctx.warnings)
            .with_human(human))
        }
        None => Ok(CommandOutcome::new(
            "day",
            json!({
                "day_key": day_key,
                "day_id": null,
                "tree": null,
                "objectives": [],
                "records": [],
            }),
        )
        .with_warnings(ctx.warnings)
        .with_human(human::day(&day_key, 0, None))),
    }
}

/// Handles `query`.
pub fn query(ctx: OperationContext, args: QueryArgs) -> Result<CommandOutcome, SillokError> {
    let from = ctx.zone.parse_timestamp(&args.from)?;
    let to = ctx.zone.parse_timestamp(&args.to)?;
    ensure_range(from, to)?;
    let tag = normalize_optional_tag(args.tag);
    let archive = ctx
        .store
        .read_or_new(ctx.recorded_at, ctx.actor(), ctx.context())?;
    let view = ChronicleView::build(&archive)?;
    let records = view.query(
        from,
        to,
        args.context.as_deref(),
        tag.as_deref(),
        args.status,
    );
    Ok(CommandOutcome::new(
        "query",
        json!({ "from": from, "to": to, "records": records }),
    )
    .with_warnings(ctx.warnings)
    .with_human(format!("{} records", records.len())))
}

/// Handles `tree`.
pub fn tree(ctx: OperationContext, args: TreeArgs) -> Result<CommandOutcome, SillokError> {
    let archive = ctx
        .store
        .read_or_new(ctx.recorded_at, ctx.actor(), ctx.context())?;
    let view = ChronicleView::build(&archive)?;
    let root_id = match args.root {
        Some(root) => ChronicleId::parse(&root)?,
        None => {
            let day_key = match args.date {
                Some(date) => ctx.zone.parse_date(&date)?,
                None => ctx.zone.day_key(ctx.event_at)?,
            };
            match view.day_id(&day_key) {
                Some(day_id) => day_id,
                None => {
                    return Ok(CommandOutcome::new(
                        "tree",
                        json!({ "root_id": null, "tree": null }),
                    )
                    .with_warnings(ctx.warnings)
                    .with_human("empty tree".to_string()));
                }
            }
        }
    };
    let tree = view.tree(root_id)?;
    Ok(
        CommandOutcome::new("tree", json!({ "root_id": root_id, "tree": tree }))
            .with_ids(json!({ "root_id": root_id }))
            .with_warnings(ctx.warnings)
            .with_human(format!("tree rooted at {root_id}")),
    )
}

/// Handles `doctor`.
pub fn doctor(ctx: OperationContext) -> Result<CommandOutcome, SillokError> {
    match ctx.store.read_existing()? {
        Some(archive) => match ChronicleView::build(&archive) {
            Ok(view) => Ok(CommandOutcome::new(
                "doctor",
                json!({
                    "valid": true,
                    "schema_version": archive.schema_version,
                    "archive_id": archive.archive_id,
                    "event_count": archive.events.len(),
                    "record_count": view.records.len(),
                    "store": ctx.store.path().display().to_string(),
                }),
            )
            .with_warnings(ctx.warnings)
            .with_human("archive valid".to_string())),
            Err(error) => Ok(CommandOutcome::new(
                "doctor",
                json!({
                    "valid": false,
                    "error": {
                        "code": error.code(),
                        "message": error.to_string(),
                    },
                    "store": ctx.store.path().display().to_string(),
                }),
            )
            .with_warnings(ctx.warnings)
            .with_human(format!("archive invalid: {error}"))),
        },
        None => Ok(CommandOutcome::new(
            "doctor",
            json!({
                "valid": true,
                "missing": true,
                "store": ctx.store.path().display().to_string(),
            }),
        )
        .with_warnings(ctx.warnings)
        .with_human("archive missing; no corruption found".to_string())),
    }
}

/// Handles `export json`.
pub fn export_json(
    ctx: OperationContext,
    args: ExportJsonArgs,
) -> Result<CommandOutcome, SillokError> {
    let archive = ctx
        .store
        .read_or_new(ctx.recorded_at, ctx.actor(), ctx.context())?;
    let view = ChronicleView::build(&archive)?;
    let records = match (args.from, args.to) {
        (Some(from), Some(to)) => {
            let from = ctx.zone.parse_timestamp(&from)?;
            let to = ctx.zone.parse_timestamp(&to)?;
            ensure_range(from, to)?;
            view.query(from, to, None, None, None)
        }
        (None, None) => view.visible_records(),
        (Some(_), None) | (None, Some(_)) => {
            return Err(SillokError::new(
                "invalid_range",
                "export json requires both --from and --to or neither",
            ));
        }
    };
    Ok(CommandOutcome::new(
        "export",
        json!({
            "archive_id": archive.archive_id,
            "records": records,
        }),
    )
    .with_ids(json!({ "archive_id": archive.archive_id }))
    .with_warnings(ctx.warnings)
    .with_human(format!("exported {} records", records.len())))
}

fn ensure_range(from: Timestamp, to: Timestamp) -> Result<(), SillokError> {
    if from <= to {
        Ok(())
    } else {
        Err(SillokError::new(
            "invalid_range",
            format!("from `{from}` is after to `{to}`"),
        ))
    }
}

fn normalize_optional_tag(value: Option<String>) -> Option<String> {
    match value {
        Some(raw) => {
            let trimmed = raw.trim().to_lowercase();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        None => None,
    }
}
