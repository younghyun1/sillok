//! One sync attempt: diff local and remote event sets, pull, and push.

use std::collections::{BTreeSet, HashMap};

use crate::domain::event::envelope::Event;
use crate::domain::id::EventId;
use crate::domain::merge;
use crate::domain::time::Timestamp;
use crate::error::SillokError;
use crate::legacy::{codec, convert};
use crate::storage::sqlite::events;
use crate::storage::sqlite::open::{Store, StoreInfo};
use crate::sync::config::SyncConfig;
use crate::sync::git::Worktree;
use crate::sync::layout::{self, EVENTS_DIR, MANIFEST_FILE, Manifest};

/// What one attempt did (or would do, for a dry run).
#[derive(Debug, Default, Clone)]
pub struct Report {
    pub pulled: usize,
    pub replaced: usize,
    pub pushed: usize,
    pub months: Vec<String>,
    pub conflicts: usize,
    pub legacy_imported: usize,
    pub commit: Option<String>,
    pub warnings: Vec<String>,
}

/// Runs one attempt. A rejected push surfaces as a retryable error.
pub fn attempt(
    store: &mut Store,
    config: &SyncConfig,
    dry_run: bool,
) -> Result<Report, SillokError> {
    let local = match events::lines(&store.conn) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let tree = match Worktree::prepare(config) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let remote = match layout::read(&tree.layout_dir()) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut report = Report::default();
    let legacy = match read_legacy(&tree, config) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    report.legacy_imported = legacy.len();

    let local_raw: HashMap<EventId, &str> = local
        .iter()
        .map(|line| (line.event_id, line.raw.as_str()))
        .collect();
    let mut incoming = legacy;
    for line in &remote.lines {
        if local_raw.get(&line.event_id) != Some(&line.raw.as_str()) {
            match Event::parse(line.raw.clone()) {
                Ok(event) => incoming.push(event),
                Err(error) => return Err(error),
            }
        }
    }
    let pull = merge::plan(&local_raw, incoming);
    report.conflicts = pull.conflicts.len();
    if report.conflicts > 0 {
        report.warnings.push(format!(
            "{} events differed between replicas; kept the canonical bytes",
            report.conflicts
        ));
    }

    // Final state per id after the pull: local, then additions, then winning replacements.
    let mut merged: HashMap<EventId, (Timestamp, &str)> = local
        .iter()
        .map(|line| (line.event_id, (line.recorded_at, line.raw.as_str())))
        .collect();
    for event in pull.additions.iter().chain(pull.replacements.iter()) {
        merged.insert(event.id(), (event.body.recorded_at, event.raw.as_str()));
    }
    let remote_raw: HashMap<EventId, &str> = remote
        .lines
        .iter()
        .map(|line| (line.event_id, line.raw.as_str()))
        .collect();
    let mut dirty: BTreeSet<String> = BTreeSet::new();
    for (id, (recorded, raw)) in &merged {
        if remote_raw.get(id) != Some(raw) {
            report.pushed += 1;
            dirty.insert(recorded.utc_month());
        }
    }
    let identity = match choose_identity(store, remote.manifest.as_ref()) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let manifest = Manifest::new(identity.archive_id, identity.created_at);
    let manifest_stale = remote.manifest.as_ref() != Some(&manifest);
    report.months = dirty.iter().cloned().collect();
    report.pulled = pull.additions.len();
    report.replaced = pull.replacements.len();
    if dry_run {
        return Ok(report);
    }

    if (report.pulled > 0 || report.replaced > 0)
        && let Err(error) = store.merge(&pull.additions, &pull.replacements)
    {
        return Err(error);
    }
    if let Err(error) = store.set_info(identity) {
        return Err(error);
    }
    let legacy_present = match &config.legacy_path {
        Some(path) => tree.root().join(path).exists(),
        None => false,
    };
    if dirty.is_empty() && !manifest_stale && !legacy_present {
        return Ok(report);
    }
    match write_layout(&tree, &merged, &dirty, &manifest, config) {
        Ok(()) => {}
        Err(error) => return Err(error),
    }
    let message = match report.months.is_empty() {
        true => "sync: update manifest".to_string(),
        false => format!(
            "sync: +{} events ({})",
            report.pushed,
            report.months.join(", ")
        ),
    };
    match tree.commit_and_push(&message) {
        Ok(commit) => {
            report.commit = commit;
            Ok(report)
        }
        Err(error) => Err(error),
    }
}

/// The older lineage wins so every replica converges on one identity.
fn choose_identity(store: &Store, manifest: Option<&Manifest>) -> Result<StoreInfo, SillokError> {
    let local = match store.info() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    match manifest {
        Some(remote)
            if (remote.created_at, remote.archive_id) < (local.created_at, local.archive_id) =>
        {
            Ok(StoreInfo {
                archive_id: remote.archive_id,
                created_at: remote.created_at,
            })
        }
        Some(_) | None => Ok(local),
    }
}

fn read_legacy(tree: &Worktree, config: &SyncConfig) -> Result<Vec<Event>, SillokError> {
    let path = match &config.legacy_path {
        Some(value) => tree.root().join(value),
        None => return Ok(Vec::new()),
    };
    let bytes = match std::fs::read(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let archive = match codec::decode_archive(&bytes) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    match convert::convert(&archive) {
        Ok(conversion) => Ok(conversion.events),
        Err(error) => Err(error),
    }
}

fn write_layout(
    tree: &Worktree,
    merged: &HashMap<EventId, (Timestamp, &str)>,
    dirty: &BTreeSet<String>,
    manifest: &Manifest,
    config: &SyncConfig,
) -> Result<(), SillokError> {
    let dir = tree.layout_dir();
    let events_dir = dir.join(EVENTS_DIR);
    if let Err(error) = std::fs::create_dir_all(&events_dir) {
        return Err(error.into());
    }
    let months = layout::by_month(
        merged
            .iter()
            .filter(|(_, (recorded, _))| dirty.contains(&recorded.utc_month()))
            .map(|(id, (recorded, raw))| (*recorded, *id, *raw)),
    );
    for (month, lines) in months {
        if let Err(error) = std::fs::write(
            events_dir.join(format!("{month}.jsonl")),
            layout::render_month(lines),
        ) {
            return Err(error.into());
        }
    }
    let encoded = match serde_json::to_string_pretty(manifest) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = std::fs::write(dir.join(MANIFEST_FILE), format!("{encoded}\n")) {
        return Err(error.into());
    }
    if let Some(legacy) = &config.legacy_path
        && let Err(error) = crate::storage::path::remove_if_exists(&tree.root().join(legacy))
    {
        return Err(error);
    }
    Ok(())
}
