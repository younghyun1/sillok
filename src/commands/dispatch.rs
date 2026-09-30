//! Routes a parsed command line to its handler.

use crate::cli::args::record::ObjectiveCommand;
use crate::cli::args::root::{Cli, Command};
use crate::cli::output::outcome::Outcome;
use crate::commands::ctx::Ctx;
use crate::commands::{admin, import, read, record};
use crate::error::SillokError;
use crate::sync::service;

/// Runs one command.
pub fn execute(cli: Cli) -> Result<Outcome, SillokError> {
    if let Command::Guide = cli.command {
        return Ok(admin::guide());
    }
    let mut ctx = match Ctx::new(cli.store, cli.tz, cli.at, cli.full) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    match cli.command {
        Command::Init => admin::init(&mut ctx),
        Command::Note(args) => record::note(&mut ctx, args),
        Command::Objective(args) => match args.command {
            ObjectiveCommand::Add(add) => record::objective_add(&mut ctx, add),
            ObjectiveCommand::Complete(complete) => record::objective_complete(&mut ctx, complete),
            ObjectiveCommand::List(list) => read::objective_list(&mut ctx, list),
        },
        Command::Amend(args) => record::amend(&mut ctx, args),
        Command::Move(args) => record::move_record(&mut ctx, args),
        Command::Retract(args) => record::retract(&mut ctx, args),
        Command::Restore(args) => record::restore(&mut ctx, &args.id),
        Command::Show(args) => read::show(&mut ctx, &args.id),
        Command::Day(args) => read::day(&mut ctx, args),
        Command::Tree(args) => read::tree(&mut ctx, args),
        Command::Query(args) => read::query(&mut ctx, args),
        Command::Status(args) => read::status(&mut ctx, args),
        Command::Export(args) => admin::export(&mut ctx, args),
        Command::Import(args) => import::import(&mut ctx, &args.path, args.dry_run),
        Command::Doctor(args) => admin::doctor(&mut ctx, args.repair),
        Command::Sync(args) => service::dispatch(&mut ctx, args.dry_run, args.command),
        Command::Reset(args) => admin::reset(&mut ctx, args.yes),
        Command::Migrate(args) => import::migrate(&mut ctx, args),
        Command::Guide => Ok(admin::guide()),
    }
}
