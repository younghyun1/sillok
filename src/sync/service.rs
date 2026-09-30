//! `sync` commands.

use std::time::Duration;

use serde_json::json;

use crate::cli::args::admin::{SyncCommand, SyncRemoteCommand, SyncRemoteSetArgs};
use crate::cli::output::outcome::Outcome;
use crate::commands::ctx::Ctx;
use crate::error::SillokError;
use crate::sync::config::{self, SyncConfig};
use crate::sync::exchange::{self, Report};

/// Attempts per sync; a push loses only to a concurrent push, so a few
/// retries with backoff are enough.
const ATTEMPTS: u32 = 3;

/// Dispatches `sync`.
pub fn dispatch(
    ctx: &mut Ctx,
    dry_run: bool,
    command: Option<SyncCommand>,
) -> Result<Outcome, SillokError> {
    match command {
        None | Some(SyncCommand::Run) => run(ctx, dry_run),
        Some(SyncCommand::Remote(args)) => match args.command {
            SyncRemoteCommand::Set(set) => remote_set(ctx, set),
            SyncRemoteCommand::Show => remote_show(ctx),
        },
    }
}

fn run(ctx: &mut Ctx, dry_run: bool) -> Result<Outcome, SillokError> {
    let config = match config::read(&ctx.store_path) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut store = match ctx.open_or_create() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut last_error = None;
    for attempt in 0..ATTEMPTS {
        match exchange::attempt(&mut store, &config, dry_run) {
            Ok(report) => return Ok(outcome(ctx, &config, report, dry_run)),
            Err(error) if error.is_retryable() && attempt + 1 < ATTEMPTS => {
                std::thread::sleep(backoff(attempt));
                last_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    match last_error {
        Some(error) => Err(error),
        None => Err(SillokError::sync("sync_git_error", "sync made no attempt")),
    }
}

/// 100 ms, 200 ms, ... plus up to 100 ms of jitter so racing replicas separate.
fn backoff(attempt: u32) -> Duration {
    let jitter = u64::from(crate::domain::id::EventId::new_v7().as_bytes()[15]) * 100 / 255;
    Duration::from_millis(100 * (1u64 << attempt) + jitter)
}

fn outcome(ctx: &mut Ctx, config: &SyncConfig, report: Report, dry_run: bool) -> Outcome {
    let mut warnings = std::mem::take(&mut ctx.warnings);
    warnings.extend(report.warnings.clone());
    let human = format!(
        "{}pulled {} (replaced {}), pushed {} in [{}]{}",
        if dry_run { "dry run: " } else { "" },
        report.pulled,
        report.replaced,
        report.pushed,
        report.months.join(", "),
        match &report.commit {
            Some(commit) => format!(", commit {commit}"),
            None => String::new(),
        }
    );
    Outcome::read(
        "sync",
        json!({
            "dry_run": dry_run,
            "remote": config.url,
            "pulled": report.pulled,
            "replaced": report.replaced,
            "pushed": report.pushed,
            "months": report.months,
            "conflicts": report.conflicts,
            "legacy_imported": report.legacy_imported,
            "commit": report.commit,
        }),
    )
    .with_human(human)
    .with_warnings(warnings)
}

fn remote_set(ctx: &mut Ctx, args: SyncRemoteSetArgs) -> Result<Outcome, SillokError> {
    let config = match SyncConfig::new(args.url, args.branch, args.dir, args.path) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    match config::write(&ctx.store_path, &config) {
        Ok(path) => Ok(Outcome::write(
            "sync",
            vec![path.display().to_string()],
            json!({ "config": config, "sidecar": path.display().to_string() }),
        )),
        Err(error) => Err(error),
    }
}

fn remote_show(ctx: &mut Ctx) -> Result<Outcome, SillokError> {
    match config::read(&ctx.store_path) {
        Ok(config) => Ok(Outcome::read(
            "sync",
            json!({ "config": config, "sidecar": config::sidecar(&ctx.store_path).display().to_string() }),
        )),
        Err(error) => Err(error),
    }
}
