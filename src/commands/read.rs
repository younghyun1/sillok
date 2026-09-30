//! Read commands: show, day, tree, query, objective list, status.

use std::collections::HashMap;

use serde_json::{Value, json};

use crate::cli::args::read::{DayArgs, QueryArgs, StatusArgs, TreeArgs};
use crate::cli::args::record::ObjectiveListArgs;
use crate::cli::human::records as human;
use crate::cli::output::outcome::Outcome;
use crate::cli::output::views;
use crate::commands::ctx::Ctx;
use crate::context::capture;
use crate::domain::id::RecordId;
use crate::domain::record::{Record, RecordKind};
use crate::domain::text;
use crate::domain::time::Timestamp;
use crate::domain::tree::build_forest;
use crate::error::SillokError;
use crate::storage::sqlite::reads::RecordFilter;

/// `show`.
pub fn show(ctx: &mut Ctx, id: &str) -> Result<Outcome, SillokError> {
    let record_id = match RecordId::parse(id) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let store = match ctx.open() {
        Ok(Some(value)) => value,
        Ok(None) => return Err(SillokError::RecordNotFound(id.to_string())),
        Err(error) => return Err(error),
    };
    let record = match store.record(record_id) {
        Ok(Some(value)) => value,
        Ok(None) => return Err(SillokError::RecordNotFound(id.to_string())),
        Err(error) => return Err(error),
    };
    let events = match store.record_events(record_id) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let children = match store.children(&[record_id]) {
        Ok(value) => value.into_iter().map(|child| child.id).collect::<Vec<_>>(),
        Err(error) => return Err(error),
    };
    let raw: Vec<Value> = events
        .iter()
        .map(|event| match serde_json::from_str::<Value>(&event.raw) {
            Ok(value) => value,
            Err(_) => Value::String(event.raw.clone()),
        })
        .collect();
    let human_text = human::show(&record, &events, &ctx.zone);
    Ok(Outcome::read(
        "show",
        json!({ "record": views::record(&record, true), "children": children, "events": raw }),
    )
    .with_human(human_text)
    .with_warnings(std::mem::take(&mut ctx.warnings)))
}

/// `day`.
pub fn day(ctx: &mut Ctx, args: DayArgs) -> Result<Outcome, SillokError> {
    let date = match ctx.date(args.date.as_deref()) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let (start, end) = match ctx.zone.day_window(date) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let (records, activity) = match ctx.open() {
        Ok(Some(store)) => match store.day(start, end) {
            Ok(value) => value,
            Err(error) => return Err(error),
        },
        Ok(None) => (Vec::new(), HashMap::new()),
        Err(error) => return Err(error),
    };
    let forest = build_forest(records, activity);
    let title = format!("{date} ({})", ctx.zone.name());
    let human_text = human::forest(&title, &forest, &ctx.zone);
    Ok(Outcome::read(
        "day",
        json!({ "date": date.to_string(), "tz": ctx.zone.name(), "records": views::forest(&forest, ctx.full) }),
    )
    .with_human(human_text)
    .with_warnings(std::mem::take(&mut ctx.warnings)))
}

/// `tree`.
pub fn tree(ctx: &mut Ctx, args: TreeArgs) -> Result<Outcome, SillokError> {
    let raw = match (args.id, args.root, args.date) {
        (Some(id), _, _) | (None, Some(id), _) => id,
        (None, None, date) => return day(ctx, DayArgs { date }),
    };
    let record_id = match RecordId::parse(&raw) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let store = match ctx.open() {
        Ok(Some(value)) => value,
        Ok(None) => return Err(SillokError::RecordNotFound(raw)),
        Err(error) => return Err(error),
    };
    let root = match store.record(record_id) {
        Ok(Some(value)) => value,
        Ok(None) => return Err(SillokError::RecordNotFound(raw)),
        Err(error) => return Err(error),
    };
    let records = match store.subtree(root) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    // The root may have a parent outside the subtree; clear it so it roots the forest.
    let records: Vec<Record> = records
        .into_iter()
        .map(|mut record| {
            if record.id == record_id {
                record.parent = None;
            }
            record
        })
        .collect();
    let forest = build_forest(records, HashMap::new());
    let human_text = human::forest(&format!("Tree {record_id}"), &forest, &ctx.zone);
    Ok(Outcome::read(
        "tree",
        json!({ "records": views::forest(&forest, ctx.full) }),
    )
    .with_human(human_text)
    .with_warnings(std::mem::take(&mut ctx.warnings)))
}

/// `query`.
pub fn query(ctx: &mut Ctx, args: QueryArgs) -> Result<Outcome, SillokError> {
    let (from, to) = match &args.date {
        Some(raw) => {
            let date = match ctx.date(Some(raw)) {
                Ok(value) => value,
                Err(error) => return Err(error),
            };
            match ctx.zone.day_window(date) {
                // --to is inclusive; the window end is the next day's first instant.
                Ok((start, end)) => (
                    Some(start),
                    Some(Timestamp::from_millis(end.as_millis() - 1)),
                ),
                Err(error) => return Err(error),
            }
        }
        None => match (
            ctx.instant(args.from.as_deref()),
            ctx.instant(args.to.as_deref()),
        ) {
            (Ok(from), Ok(to)) => (from, to),
            (Err(error), _) | (_, Err(error)) => return Err(error),
        },
    };
    if let (Some(start), Some(end)) = (from, to)
        && start > end
    {
        return Err(SillokError::invalid(
            "invalid_range",
            format!("--from {start} is after --to {end}"),
        ));
    }
    let tags = match text::tags(args.tags) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let filter = RecordFilter {
        from,
        to,
        tags,
        status: args.status.map(|status| status.status()),
        kind: args.kind.map(|kind| kind.kind()),
        open_only: args.open,
        text: args.text,
        context: args.context,
        context_key: None,
        limit: args.limit,
    };
    list(ctx, "query", "Query", &filter)
}

/// `objective list`.
pub fn objective_list(ctx: &mut Ctx, args: ObjectiveListArgs) -> Result<Outcome, SillokError> {
    let filter = RecordFilter {
        kind: Some(RecordKind::Objective),
        open_only: !args.all,
        limit: args.limit,
        ..RecordFilter::default()
    };
    list(ctx, "objective", "Objectives", &filter)
}

/// `status`: what an agent needs to resume work in this repository.
pub fn status(ctx: &mut Ctx, args: StatusArgs) -> Result<Outcome, SillokError> {
    let key = match args.all {
        true => None,
        false => capture::current_key(),
    };
    let store = match ctx.open() {
        Ok(Some(value)) => value,
        Ok(None) => {
            return Ok(Outcome::read(
                "status",
                json!({ "context": key, "objectives": [], "recent": [] }),
            ));
        }
        Err(error) => return Err(error),
    };
    let recent = match store.recent(key.as_deref(), args.limit) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    // Objectives created in this repository, filtered in SQL before the
    // limit, plus open objectives that parent recent work done here.
    let mut objectives = match store.query(&RecordFilter {
        kind: Some(RecordKind::Objective),
        open_only: true,
        context_key: key.clone(),
        limit: 50,
        ..RecordFilter::default()
    }) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut parent_ids: Vec<RecordId> = recent
        .iter()
        .filter_map(|record| record.parent)
        .filter(|parent| !objectives.iter().any(|objective| objective.id == *parent))
        .collect();
    parent_ids.sort();
    parent_ids.dedup();
    match store.records(&parent_ids) {
        Ok(parents) => objectives.extend(
            parents
                .into_iter()
                .filter(|record| record.kind == RecordKind::Objective && record.status.is_open()),
        ),
        Err(error) => return Err(error),
    }
    objectives.sort_by_key(|record| (record.created_at, record.id));
    let human_text = format!(
        "{}\n\n{}",
        human::list("Open objectives", &objectives, &ctx.zone),
        human::list("Recent", &recent, &ctx.zone)
    );
    Ok(Outcome::read(
        "status",
        json!({
            "context": key,
            "objectives": views::records(&objectives, ctx.full),
            "recent": views::records(&recent, ctx.full),
        }),
    )
    .with_human(human_text)
    .with_warnings(std::mem::take(&mut ctx.warnings)))
}

fn list(
    ctx: &mut Ctx,
    command: &'static str,
    title: &str,
    filter: &RecordFilter,
) -> Result<Outcome, SillokError> {
    let records = match ctx.open() {
        Ok(Some(store)) => match store.query(filter) {
            Ok(value) => value,
            Err(error) => return Err(error),
        },
        Ok(None) => Vec::new(),
        Err(error) => return Err(error),
    };
    let human_text = human::list(title, &records, &ctx.zone);
    Ok(Outcome::read(
        command,
        json!({ "records": views::records(&records, ctx.full) }),
    )
    .with_human(human_text)
    .with_warnings(std::mem::take(&mut ctx.warnings)))
}
