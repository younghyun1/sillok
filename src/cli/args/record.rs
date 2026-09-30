//! Arguments of the commands that write records.

use clap::{Args, Subcommand, ValueEnum};

use crate::domain::record::{RecordKind, RecordStatus};

/// Status values accepted on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusArg {
    Open,
    Active,
    Blocked,
    #[value(alias = "done")]
    Completed,
    /// Only for `query`; writes use `retract`.
    Retracted,
}

impl StatusArg {
    /// Domain status.
    pub fn status(self) -> RecordStatus {
        match self {
            Self::Open => RecordStatus::Open,
            Self::Active => RecordStatus::Active,
            Self::Blocked => RecordStatus::Blocked,
            Self::Completed => RecordStatus::Completed,
            Self::Retracted => RecordStatus::Retracted,
        }
    }
}

/// Record kinds accepted on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum KindArg {
    Objective,
    Task,
}

impl KindArg {
    /// Domain kind.
    pub fn kind(self) -> RecordKind {
        match self {
            Self::Objective => RecordKind::Objective,
            Self::Task => RecordKind::Task,
        }
    }
}

/// `note`.
#[derive(Debug, Args)]
pub struct NoteArgs {
    /// What was done or noticed.
    pub text: String,
    /// Objective or task this belongs under.
    #[arg(long)]
    pub parent: Option<String>,
    /// Why it mattered.
    #[arg(long)]
    pub purpose: Option<String>,
    /// Comma-separated tags.
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,
    #[arg(long, value_enum, default_value_t = StatusArg::Completed)]
    pub status: StatusArg,
}

/// `objective`.
#[derive(Debug, Args)]
pub struct ObjectiveArgs {
    #[command(subcommand)]
    pub command: ObjectiveCommand,
}

/// `objective` subcommands.
#[derive(Debug, Subcommand)]
pub enum ObjectiveCommand {
    /// Start an objective (default status: active). Prints its id.
    Add(ObjectiveAddArgs),
    /// Mark an objective completed.
    Complete(ObjectiveCompleteArgs),
    /// Open objectives, newest last.
    List(ObjectiveListArgs),
}

/// `objective add`.
#[derive(Debug, Args)]
pub struct ObjectiveAddArgs {
    pub text: String,
    /// Parent objective.
    #[arg(long)]
    pub parent: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,
    #[arg(long, value_enum, default_value_t = StatusArg::Active)]
    pub status: StatusArg,
}

/// `objective complete`.
#[derive(Debug, Args)]
pub struct ObjectiveCompleteArgs {
    pub id: String,
    /// Outcome summary.
    #[arg(long)]
    pub note: Option<String>,
}

/// `objective list`.
#[derive(Debug, Args)]
pub struct ObjectiveListArgs {
    /// Include completed objectives.
    #[arg(long)]
    pub all: bool,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

/// `amend`.
#[derive(Debug, Args)]
pub struct AmendArgs {
    pub id: String,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long, value_enum)]
    pub status: Option<StatusArg>,
    #[arg(long, conflicts_with = "clear_purpose")]
    pub purpose: Option<String>,
    #[arg(long)]
    pub clear_purpose: bool,
    /// Replaces all tags.
    #[arg(long, value_delimiter = ',', conflicts_with = "clear_tags")]
    pub tags: Vec<String>,
    #[arg(long)]
    pub clear_tags: bool,
    /// Outcome or remark attached to this change.
    #[arg(long)]
    pub note: Option<String>,
}

/// `move`.
#[derive(Debug, Args)]
pub struct MoveArgs {
    pub id: String,
    /// New parent.
    #[arg(long, required_unless_present = "top", conflicts_with = "top")]
    pub parent: Option<String>,
    /// Make the record top-level.
    #[arg(long)]
    pub top: bool,
}

/// `retract`.
#[derive(Debug, Args)]
pub struct RetractArgs {
    pub id: String,
    #[arg(long)]
    pub reason: String,
}

/// A single record id.
#[derive(Debug, Args)]
pub struct IdArgs {
    pub id: String,
}
