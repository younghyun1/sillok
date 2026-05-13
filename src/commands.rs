use crate::cli::args::{Cli, Command, ExportCommand, ObjectiveCommand};
use crate::cli::output::CommandOutcome;
use crate::error::SillokError;
use crate::operation::OperationContext;

/// Dispatches one parsed CLI command.
pub fn execute(cli: Cli) -> Result<CommandOutcome, SillokError> {
    let ctx = OperationContext::new(cli.store, cli.at, cli.tz)?;
    match cli.command {
        Command::Init => crate::mutations::init(ctx),
        Command::Note(args) => crate::mutations::note(ctx, args),
        Command::Objective(args) => match args.command {
            ObjectiveCommand::Add(args) => crate::mutations::objective_add(ctx, args),
            ObjectiveCommand::Complete(args) => crate::mutations::objective_complete(ctx, args),
        },
        Command::Amend(args) => crate::mutations::amend(ctx, args),
        Command::Retract(args) => crate::mutations::retract(ctx, args),
        Command::Show(args) => crate::queries::show(ctx, args),
        Command::Day(args) => crate::queries::day(ctx, args),
        Command::Query(args) => crate::queries::query(ctx, args),
        Command::Tree(args) => crate::queries::tree(ctx, args),
        Command::Doctor => crate::queries::doctor(ctx),
        Command::Export(args) => match args.command {
            ExportCommand::Json(args) => crate::queries::export_json(ctx, args),
        },
        Command::Truncate(args) => crate::mutations::truncate(ctx, args),
    }
}
