use crate::cli::args::{
    Cli, Command, ExportCommand, ObjectiveCommand, SyncCommand, SyncRemoteCommand,
};
use crate::cli::output::{CommandOutcome, OutputMode};
use crate::error::SillokError;
use crate::operation::OperationContext;

/// Dispatches one parsed CLI command.
pub async fn execute(cli: Cli) -> Result<CommandOutcome, SillokError> {
    let store_path = cli.store.clone();
    let at = cli.at.clone();
    let tz = cli.tz.clone();
    let output_mode = OutputMode::from_flags(cli.human, cli.json);
    let command = match cli.command {
        Command::Migrate(args) => {
            return crate::migration::migrate(store_path, at, tz, output_mode, args).await;
        }
        other => other,
    };
    let ctx = OperationContext::new(cli.store, cli.at, cli.tz, output_mode)?;
    match command {
        Command::Init => crate::mutations::init(ctx).await,
        Command::Note(args) => crate::mutations::note(ctx, args).await,
        Command::Objective(args) => match args.command {
            ObjectiveCommand::Add(args) => crate::mutations::objective_add(ctx, args).await,
            ObjectiveCommand::Complete(args) => {
                crate::mutations::objective_complete(ctx, args).await
            }
        },
        Command::Amend(args) => crate::mutations::amend(ctx, args).await,
        Command::Retract(args) => crate::mutations::retract(ctx, args).await,
        Command::Show(args) => crate::queries::show(ctx, args).await,
        Command::Day(args) => crate::queries::day(ctx, args).await,
        Command::Query(args) => crate::queries::query(ctx, args).await,
        Command::Tree(args) => crate::queries::tree(ctx, args).await,
        Command::Doctor => crate::queries::doctor(ctx).await,
        Command::Export(args) => match args.command {
            ExportCommand::Json(args) => crate::queries::export_json(ctx, args).await,
        },
        Command::Sync(args) => match args.command {
            SyncCommand::Remote(args) => match args.command {
                SyncRemoteCommand::Set(args) => crate::sync::service::remote_set(ctx, args).await,
                SyncRemoteCommand::Show => crate::sync::service::remote_show(ctx).await,
            },
            SyncCommand::Pull => crate::sync::service::pull(ctx).await,
            SyncCommand::Push => crate::sync::service::push(ctx).await,
            SyncCommand::Run => crate::sync::service::run(ctx).await,
        },
        Command::Truncate(args) => crate::mutations::truncate(ctx, args).await,
        Command::Migrate(_) => Err(SillokError::new(
            "invalid_command",
            "migrate dispatch failed",
        )),
    }
}
