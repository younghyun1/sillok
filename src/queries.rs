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
pub async fn show(ctx: OperationContext, args: IdArgs) -> Result<CommandOutcome, SillokError> {
    reject_legacy_read(&ctx, "show")?;
    let record_id = ChronicleId::parse(&args.id)?;
    let store = ctx.store.sql();
    let (record, events) = match store.show(record_id).await? {
        Some(value) => value,
        None => {
            return Err(SillokError::new(
                "record_not_found",
                format!("record `{record_id}` does not exist"),
            ));
        }
    };
    let human = human::show(&record, &events);
    Ok(
        CommandOutcome::new("show", json!({ "record": record, "events": events }))
            .with_ids(json!({ "record_id": record_id }))
            .with_warnings(ctx.warnings)
            .with_human(human),
    )
}

/// Handles `day`.
pub async fn day(ctx: OperationContext, args: DayArgs) -> Result<CommandOutcome, SillokError> {
    reject_legacy_read(&ctx, "day")?;
    let day_key = match args.date {
        Some(date) => ctx.zone.parse_date(&date)?,
        None => ctx.zone.day_key(ctx.event_at)?,
    };
    match ctx.store.sql().day(&day_key).await? {
        Some((day_id, tree, records)) => {
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
pub async fn query(ctx: OperationContext, args: QueryArgs) -> Result<CommandOutcome, SillokError> {
    reject_legacy_read(&ctx, "query")?;
    let from = ctx.zone.parse_timestamp(&args.from)?;
    let to = ctx.zone.parse_timestamp(&args.to)?;
    ensure_range(from, to)?;
    let tag = normalize_optional_tag(args.tag);
    let records = ctx
        .store
        .sql()
        .query_records(
            from,
            to,
            args.context.as_deref(),
            tag.as_deref(),
            args.status,
        )
        .await?;
    let human = human::records(&format!("Query {from} to {to}"), &records);
    Ok(CommandOutcome::new(
        "query",
        json!({ "from": from, "to": to, "records": records }),
    )
    .with_warnings(ctx.warnings)
    .with_human(human))
}

/// Handles `tree`.
pub async fn tree(ctx: OperationContext, args: TreeArgs) -> Result<CommandOutcome, SillokError> {
    reject_legacy_read(&ctx, "tree")?;
    let store = ctx.store.sql();
    let root_id = match args.root {
        Some(root) => ChronicleId::parse(&root)?,
        None => {
            let day_key = match args.date {
                Some(date) => ctx.zone.parse_date(&date)?,
                None => ctx.zone.day_key(ctx.event_at)?,
            };
            match store.day(&day_key).await? {
                Some((day_id, _, _)) => day_id,
                None => {
                    return Ok(CommandOutcome::new(
                        "tree",
                        json!({ "root_id": null, "tree": null }),
                    )
                    .with_warnings(ctx.warnings)
                    .with_human(human::tree(None, None)));
                }
            }
        }
    };
    let tree = match store.tree(root_id).await? {
        Some(value) => value,
        None => {
            return Err(SillokError::new(
                "record_not_found",
                format!("record `{root_id}` does not exist"),
            ));
        }
    };
    let human = human::tree(Some(root_id), Some(&tree));
    Ok(
        CommandOutcome::new("tree", json!({ "root_id": root_id, "tree": tree }))
            .with_ids(json!({ "root_id": root_id }))
            .with_warnings(ctx.warnings)
            .with_human(human),
    )
}

/// Handles `doctor`.
pub async fn doctor(ctx: OperationContext) -> Result<CommandOutcome, SillokError> {
    let checked_at = ctx.recorded_at;
    if ctx.store.is_legacy_path() {
        return legacy_doctor(ctx, checked_at);
    }
    match ctx.store.sql().doctor().await {
        Ok(Some(stats)) => {
            let human = human::store_doctor_valid(
                stats.info.archive_id,
                stats.info.created_at,
                stats.event_count,
                stats.record_count,
                ctx.store.path(),
                checked_at,
            );
            Ok(CommandOutcome::new(
                "doctor",
                json!({
                    "valid": true,
                    "checked_at": checked_at,
                    "schema_version": crate::domain::archive::ARCHIVE_SCHEMA_VERSION,
                    "store_datashape_version": crate::storage::sql::schema::STORE_DATASHAPE_VERSION,
                    "archive_id": stats.info.archive_id,
                    "created_at": stats.info.created_at,
                    "event_count": stats.event_count,
                    "record_count": stats.record_count,
                    "store": ctx.store.path().display().to_string(),
                }),
            )
            .with_warnings(ctx.warnings)
            .with_human(human))
        }
        Ok(None) => Ok(CommandOutcome::new(
            "doctor",
            json!({
                "valid": true,
                "missing": true,
                "checked_at": checked_at,
                "store": ctx.store.path().display().to_string(),
            }),
        )
        .with_warnings(ctx.warnings)
        .with_human(human::doctor_missing(ctx.store.path(), checked_at))),
        Err(error) => Ok(CommandOutcome::new(
            "doctor",
            json!({
                "valid": false,
                "checked_at": checked_at,
                "error": {
                    "code": error.code(),
                    "message": error.to_string(),
                },
                "store": ctx.store.path().display().to_string(),
            }),
        )
        .with_warnings(ctx.warnings)
        .with_human(human::doctor_invalid(&error, ctx.store.path(), checked_at))),
    }
}

fn legacy_doctor(
    ctx: OperationContext,
    checked_at: Timestamp,
) -> Result<CommandOutcome, SillokError> {
    match ctx.store.legacy().read_existing()? {
        Some(archive) => match ChronicleView::build(&archive) {
            Ok(view) => {
                let human =
                    human::doctor_valid(&archive, view.records.len(), ctx.store.path(), checked_at);
                Ok(CommandOutcome::new(
                    "doctor",
                    json!({
                        "valid": true,
                        "checked_at": checked_at,
                        "schema_version": archive.schema_version,
                        "archive_id": archive.archive_id,
                        "created_at": archive.created_at,
                        "event_count": archive.events.len(),
                        "record_count": view.records.len(),
                        "store": ctx.store.path().display().to_string(),
                    }),
                )
                .with_warnings(ctx.warnings)
                .with_human(human))
            }
            Err(error) => Ok(CommandOutcome::new(
                "doctor",
                json!({
                    "valid": false,
                    "checked_at": checked_at,
                    "error": {
                        "code": error.code(),
                        "message": error.to_string(),
                    },
                    "store": ctx.store.path().display().to_string(),
                }),
            )
            .with_warnings(ctx.warnings)
            .with_human(human::doctor_invalid(&error, ctx.store.path(), checked_at))),
        },
        None => Ok(CommandOutcome::new(
            "doctor",
            json!({
                "valid": true,
                "missing": true,
                "checked_at": checked_at,
                "store": ctx.store.path().display().to_string(),
            }),
        )
        .with_warnings(ctx.warnings)
        .with_human(human::doctor_missing(ctx.store.path(), checked_at))),
    }
}

/// Handles `export json`.
pub async fn export_json(
    ctx: OperationContext,
    args: ExportJsonArgs,
) -> Result<CommandOutcome, SillokError> {
    if ctx.store.is_legacy_path() {
        return legacy_export_json(ctx, args);
    }
    let records = match (args.from, args.to) {
        (Some(from), Some(to)) => {
            let from = ctx.zone.parse_timestamp(&from)?;
            let to = ctx.zone.parse_timestamp(&to)?;
            ensure_range(from, to)?;
            ctx.store
                .sql()
                .query_records(from, to, None, None, None)
                .await?
        }
        (None, None) => ctx.store.sql().visible_records().await?,
        (Some(_), None) | (None, Some(_)) => {
            return Err(SillokError::new(
                "invalid_range",
                "export json requires both --from and --to or neither",
            ));
        }
    };
    let archive_id = ctx
        .store
        .sql()
        .stats()
        .await?
        .map(|stats| stats.info.archive_id);
    let human = human::records("Export", &records);
    Ok(CommandOutcome::new(
        "export",
        json!({
            "archive_id": archive_id,
            "records": records,
        }),
    )
    .with_ids(json!({ "archive_id": archive_id }))
    .with_warnings(ctx.warnings)
    .with_human(human))
}

fn legacy_export_json(
    ctx: OperationContext,
    args: ExportJsonArgs,
) -> Result<CommandOutcome, SillokError> {
    let archive = ctx
        .store
        .legacy()
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
    let human = human::records("Export", &records);
    Ok(CommandOutcome::new(
        "export",
        json!({
            "archive_id": archive.archive_id,
            "records": records,
        }),
    )
    .with_ids(json!({ "archive_id": archive.archive_id }))
    .with_warnings(ctx.warnings)
    .with_human(human))
}

fn reject_legacy_read(ctx: &OperationContext, command: &'static str) -> Result<(), SillokError> {
    if ctx.store.is_legacy_path() {
        Err(SillokError::new(
            "migration_required",
            format!("`{command}` requires a v2 store; run `sillok migrate` for legacy archives"),
        ))
    } else {
        Ok(())
    }
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
