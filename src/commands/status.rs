//! `status`: open objectives and recent work for the current repository.

use serde_json::{Value, json};

use crate::cli::args::read::StatusArgs;
use crate::cli::human::records as human;
use crate::cli::output::outcome::Outcome;
use crate::cli::output::views;
use crate::commands::ctx::Ctx;
use crate::context::capture;
use crate::domain::id::RecordId;
use crate::domain::record::RecordKind;
use crate::error::SillokError;
use crate::skill::install;
use crate::storage::sqlite::reads::RecordFilter;
use crate::sync::auto;

/// `status`: what an agent needs to resume work in this repository.
pub fn status(ctx: &mut Ctx, args: StatusArgs) -> Result<Outcome, SillokError> {
    let scope = match args.all {
        true => None,
        false => capture::current_scope(),
    };
    // Session start is the one moment every agent runs this command, so it
    // is where an installed skill left behind by an upgrade gets refreshed.
    ctx.warnings.extend(install::refresh());
    // It is also the moment to exchange events with the other replicas,
    // in the background so resuming never waits on the network.
    if let Some(notice) = auto::maybe_start(&ctx.store_path, ctx.now) {
        ctx.warnings.push(notice);
    }
    let (context, remote) = match &scope {
        Some(value) => (Some(value.root.clone()), value.remote.clone()),
        None => (None, None),
    };
    let store = match ctx.open() {
        Ok(Some(value)) => value,
        Ok(None) => {
            return Ok(
                Outcome::read("status", status_data(context, remote, json!([]), json!([])))
                    .with_warnings(std::mem::take(&mut ctx.warnings)),
            );
        }
        Err(error) => return Err(error),
    };
    let recent = match store.recent(scope.as_ref(), args.limit) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    // Objectives created in this repository, filtered in SQL before the
    // limit, plus open objectives that parent recent work done here.
    let mut objectives = match store.query(&RecordFilter {
        kind: Some(RecordKind::Objective),
        open_only: true,
        scope,
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
        status_data(
            context,
            remote,
            views::records(&objectives, ctx.full),
            views::records(&recent, ctx.full),
        ),
    )
    .with_human(human_text)
    .with_warnings(std::mem::take(&mut ctx.warnings)))
}

/// `status` payload; `remote` appears only when the repository has one.
fn status_data(
    context: Option<String>,
    remote: Option<String>,
    objectives: Value,
    recent: Value,
) -> Value {
    let mut data = json!({ "context": context });
    if let Value::Object(map) = &mut data {
        if let Some(url) = remote {
            map.insert("remote".to_string(), json!(url));
        }
        map.insert("objectives".to_string(), objectives);
        map.insert("recent".to_string(), recent);
    }
    data
}
