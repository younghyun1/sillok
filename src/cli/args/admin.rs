//! Arguments of the maintenance and sync commands.

use std::path::PathBuf;

use clap::{Args, Subcommand};

/// `import`.
#[derive(Debug, Args)]
pub struct ImportArgs {
    /// 0.x store or `.slk.zst` archive, events JSONL, or sync directory.
    pub path: PathBuf,
    /// Report what would be merged without writing.
    #[arg(long)]
    pub dry_run: bool,
}

/// `migrate` (0.10): imports `--store` into `--target`.
#[derive(Debug, Args)]
pub struct MigrateArgs {
    #[arg(long)]
    pub target: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub yes: bool,
}

/// `doctor`.
#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Rebuild the projection from events.
    #[arg(long)]
    pub repair: bool,
}

/// `reset`.
#[derive(Debug, Args)]
pub struct ResetArgs {
    /// Required confirmation.
    #[arg(long)]
    pub yes: bool,
}

/// `sync`: without a subcommand, exchanges events with the remote.
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Report what would be pulled and pushed without writing.
    #[arg(long)]
    pub dry_run: bool,
    #[command(subcommand)]
    pub command: Option<SyncCommand>,
}

/// `sync` subcommands.
#[derive(Debug, Subcommand)]
pub enum SyncCommand {
    /// Configure the Git remote.
    Remote(SyncRemoteArgs),
    /// 0.10 spelling of bare `sync`.
    #[command(hide = true)]
    Run,
}

/// `sync remote`.
#[derive(Debug, Args)]
pub struct SyncRemoteArgs {
    #[command(subcommand)]
    pub command: SyncRemoteCommand,
}

/// `sync remote` subcommands.
#[derive(Debug, Subcommand)]
pub enum SyncRemoteCommand {
    /// Set the remote URL, branch, and directory.
    Set(SyncRemoteSetArgs),
    /// Print the configuration.
    Show,
}

/// `sync remote set`.
#[derive(Debug, Args)]
pub struct SyncRemoteSetArgs {
    pub url: String,
    /// Default: main.
    #[arg(long)]
    pub branch: Option<String>,
    /// Directory inside the repository. Default: sillok.
    #[arg(long)]
    pub dir: Option<String>,
    /// 0.10 artifact path; imported and removed on the next sync.
    #[arg(long, hide = true)]
    pub path: Option<String>,
}
