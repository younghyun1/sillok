use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::domain::event::RecordStatus;

/// Sillok records meticulous daily agent work chronicles.
#[derive(Debug, Parser)]
#[command(name = "sillok", version, about)]
pub struct Cli {
    /// Archive store path. Defaults to SILLOK_STORE or XDG data-local storage.
    #[arg(long, global = true, env = "SILLOK_STORE")]
    pub store: Option<PathBuf>,

    /// Print readable summaries instead of JSON.
    #[arg(long, global = true)]
    pub human: bool,

    /// Accepted for explicitness; JSON is the default output.
    #[arg(long, global = true)]
    pub json: bool,

    /// Event timestamp for backfills. RFC3339 or YYYY-MM-DDTHH:MM:SS.
    #[arg(long, global = true)]
    pub at: Option<String>,

    /// Timezone used for local day attribution and naive --at parsing.
    #[arg(long, global = true)]
    pub tz: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

/// Top-level command set.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize the archive if absent.
    Init,
    /// Record a completed task or work note.
    Note(NoteArgs),
    /// Manage day objectives.
    Objective(ObjectiveArgs),
    /// Amend the current derived state of a record.
    Amend(AmendArgs),
    /// Retract a record from current views.
    Retract(RetractArgs),
    /// Show one record and its event history.
    Show(IdArgs),
    /// Summarize a day.
    Day(DayArgs),
    /// Query current records by creation time.
    Query(QueryArgs),
    /// Render a task tree.
    Tree(TreeArgs),
    /// Validate archive integrity.
    Doctor,
    /// Export current derived data.
    Export(ExportArgs),
    /// Backup and reset the whole archive.
    Truncate(TruncateArgs),
}

impl Command {
    /// Stable command name for JSON responses.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Note(_) => "note",
            Self::Objective(_) => "objective",
            Self::Amend(_) => "amend",
            Self::Retract(_) => "retract",
            Self::Show(_) => "show",
            Self::Day(_) => "day",
            Self::Query(_) => "query",
            Self::Tree(_) => "tree",
            Self::Doctor => "doctor",
            Self::Export(_) => "export",
            Self::Truncate(_) => "truncate",
        }
    }
}

/// Args for `note`.
#[derive(Debug, Args)]
pub struct NoteArgs {
    pub text: String,
    #[arg(long)]
    pub parent: Option<String>,
    #[arg(long)]
    pub purpose: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,
    #[arg(long, value_enum, default_value_t = RecordStatus::Completed)]
    pub status: RecordStatus,
}

/// Args for `objective`.
#[derive(Debug, Args)]
pub struct ObjectiveArgs {
    #[command(subcommand)]
    pub command: ObjectiveCommand,
}

/// Objective subcommands.
#[derive(Debug, Subcommand)]
pub enum ObjectiveCommand {
    Add(ObjectiveAddArgs),
    Complete(ObjectiveCompleteArgs),
}

/// Args for `objective add`.
#[derive(Debug, Args)]
pub struct ObjectiveAddArgs {
    pub text: String,
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,
}

/// Args for `objective complete`.
#[derive(Debug, Args)]
pub struct ObjectiveCompleteArgs {
    pub id: String,
    #[arg(long)]
    pub note: Option<String>,
}

/// Args for `amend`.
#[derive(Debug, Args)]
pub struct AmendArgs {
    pub id: String,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long, value_enum)]
    pub status: Option<RecordStatus>,
    #[arg(long)]
    pub purpose: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,
}

/// Args for `retract`.
#[derive(Debug, Args)]
pub struct RetractArgs {
    pub id: String,
    #[arg(long)]
    pub reason: String,
}

/// Args containing a single id.
#[derive(Debug, Args)]
pub struct IdArgs {
    pub id: String,
}

/// Args for `day`.
#[derive(Debug, Args)]
pub struct DayArgs {
    #[arg(long)]
    pub date: Option<String>,
}

/// Args for `query`.
#[derive(Debug, Args)]
pub struct QueryArgs {
    #[arg(long)]
    pub from: String,
    #[arg(long)]
    pub to: String,
    #[arg(long)]
    pub context: Option<String>,
    #[arg(long)]
    pub tag: Option<String>,
    #[arg(long, value_enum)]
    pub status: Option<RecordStatus>,
}

/// Args for `tree`.
#[derive(Debug, Args)]
pub struct TreeArgs {
    #[arg(long)]
    pub date: Option<String>,
    #[arg(long)]
    pub root: Option<String>,
}

/// Args for `export`.
#[derive(Debug, Args)]
pub struct ExportArgs {
    #[command(subcommand)]
    pub command: ExportCommand,
}

/// Export subcommands.
#[derive(Debug, Subcommand)]
pub enum ExportCommand {
    Json(ExportJsonArgs),
}

/// Args for `export json`.
#[derive(Debug, Args)]
pub struct ExportJsonArgs {
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
}

/// Args for `truncate`.
#[derive(Debug, Args)]
pub struct TruncateArgs {
    /// Required confirmation for destructive reset.
    #[arg(long)]
    pub yes: bool,
}
