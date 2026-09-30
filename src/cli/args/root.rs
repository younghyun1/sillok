//! Top-level command line.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::cli::args::admin::{DoctorArgs, ImportArgs, MigrateArgs, ResetArgs, SyncArgs};
use crate::cli::args::read::{DayArgs, ExportArgs, QueryArgs, StatusArgs, TreeArgs};
use crate::cli::args::record::{AmendArgs, IdArgs, MoveArgs, NoteArgs, ObjectiveArgs, RetractArgs};

const AFTER_HELP: &str =
    "Writes print the affected id; reads print compact JSON; errors are JSON on stderr.
Run `sillok guide` for the agent workflow. Environment: SILLOK_STORE, SILLOK_TZ,
SILLOK_ACTOR, SILLOK_SESSION, SILLOK_OUTPUT (compact|json|human), SILLOK_LOG.";

/// Structured chronicle of agent work: objectives, tasks, and what happened each day.
#[derive(Debug, Parser)]
#[command(name = "sillok", version, about, after_help = AFTER_HELP)]
pub struct Cli {
    /// Store path. Defaults to SILLOK_STORE, then the XDG data directory.
    #[arg(long, global = true, env = "SILLOK_STORE")]
    pub store: Option<PathBuf>,

    /// Print the full JSON envelope.
    #[arg(long, global = true, conflicts_with = "human")]
    pub json: bool,

    /// Print readable text for people.
    #[arg(long, global = true)]
    pub human: bool,

    /// Include each record's work context (cwd, Git state, session).
    #[arg(long, global = true)]
    pub full: bool,

    /// When the work happened: RFC 3339, or YYYY-MM-DDTHH:MM[:SS] in --tz.
    #[arg(long, global = true)]
    pub at: Option<String>,

    /// IANA timezone for day windows and naive times. Defaults to SILLOK_TZ, then the system zone.
    #[arg(long, global = true, env = "SILLOK_TZ")]
    pub tz: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

/// Commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create the store if missing and print its archive id.
    Init,
    /// Record a task (default status: completed). Prints the task id.
    Note(NoteArgs),
    /// Add, complete, or list objectives.
    Objective(ObjectiveArgs),
    /// Change fields of a record.
    Amend(AmendArgs),
    /// Put a record under another parent, or make it top-level.
    Move(MoveArgs),
    /// Hide a record from views, keeping its history.
    Retract(RetractArgs),
    /// Undo a retraction.
    Restore(IdArgs),
    /// One record with its full event history.
    Show(IdArgs),
    /// Everything that happened on a day, as a tree.
    Day(DayArgs),
    /// A record and its visible descendants.
    Tree(TreeArgs),
    /// Search records by time, tag, status, kind, text, or context.
    Query(QueryArgs),
    /// Open objectives and recent work for the current repository.
    Status(StatusArgs),
    /// Stream records or raw events as JSON lines.
    Export(ExportArgs),
    /// Merge events from a 0.x store or archive, a JSONL export, or a sync directory.
    Import(ImportArgs),
    /// Check integrity and that the projection matches a replay.
    Doctor(DoctorArgs),
    /// Share events through a Git remote.
    Sync(SyncArgs),
    /// Back up the store and start empty.
    #[command(alias = "truncate")]
    Reset(ResetArgs),
    /// Print the agent usage guide.
    Guide,
    /// 0.10 spelling of `import`; kept so old instructions keep working.
    #[command(hide = true)]
    Migrate(MigrateArgs),
}

impl Command {
    /// Stable command name used in JSON envelopes and errors.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Note(_) => "note",
            Self::Objective(_) => "objective",
            Self::Amend(_) => "amend",
            Self::Move(_) => "move",
            Self::Retract(_) => "retract",
            Self::Restore(_) => "restore",
            Self::Show(_) => "show",
            Self::Day(_) => "day",
            Self::Tree(_) => "tree",
            Self::Query(_) => "query",
            Self::Status(_) => "status",
            Self::Export(_) => "export",
            Self::Import(_) => "import",
            Self::Doctor(_) => "doctor",
            Self::Sync(_) => "sync",
            Self::Reset(_) => "reset",
            Self::Guide => "guide",
            Self::Migrate(_) => "migrate",
        }
    }
}
