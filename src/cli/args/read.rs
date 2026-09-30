//! Arguments of the read commands.

use clap::{Args, Subcommand};

use crate::cli::args::record::{KindArg, StatusArg};

/// `day`.
#[derive(Debug, Args)]
pub struct DayArgs {
    /// YYYY-MM-DD in --tz; defaults to today.
    #[arg(long)]
    pub date: Option<String>,
}

/// `tree`.
#[derive(Debug, Args)]
pub struct TreeArgs {
    /// Root record id.
    pub id: Option<String>,
    /// 0.10 spelling of the root id.
    #[arg(long, hide = true)]
    pub root: Option<String>,
    /// 0.10 day tree; now the same as `day --date`.
    #[arg(long, hide = true)]
    pub date: Option<String>,
}

/// `query`.
#[derive(Debug, Args)]
pub struct QueryArgs {
    /// Created at or after (RFC 3339 or naive in --tz).
    #[arg(long, alias = "since", conflicts_with = "date")]
    pub from: Option<String>,
    /// Created at or before; defaults to now.
    #[arg(long, conflicts_with = "date")]
    pub to: Option<String>,
    /// Created on this local date.
    #[arg(long)]
    pub date: Option<String>,
    /// Required tag; repeat or comma-separate for several.
    #[arg(long = "tag", value_delimiter = ',')]
    pub tags: Vec<String>,
    #[arg(long, value_enum)]
    pub status: Option<StatusArg>,
    #[arg(long, value_enum)]
    pub kind: Option<KindArg>,
    /// Only open, active, or blocked records.
    #[arg(long)]
    pub open: bool,
    /// Case-insensitive text substring.
    #[arg(long)]
    pub text: Option<String>,
    /// Substring of the repository root or working directory.
    #[arg(long)]
    pub context: Option<String>,
    /// Keep the newest N matches.
    #[arg(long, default_value_t = 100)]
    pub limit: usize,
}

/// `status`.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Every repository, not just the current one.
    #[arg(long)]
    pub all: bool,
    /// Recent records to show.
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
}

/// `export`.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ExportArgs {
    /// Raw events (the portable archive) instead of records.
    #[arg(long)]
    pub events: bool,
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
    #[command(subcommand)]
    pub format: Option<ExportFormat>,
}

/// 0.10 `export json`; kept as an alias of `export`.
#[derive(Debug, Subcommand)]
pub enum ExportFormat {
    #[command(hide = true)]
    Json(ExportRange),
}

/// Range arguments of the 0.10 alias.
#[derive(Debug, Args)]
pub struct ExportRange {
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
}
