use clap::Parser;

use crate::cli::args::Cli;
use crate::cli::output::{print_failure, print_success};
use crate::commands::execute;

/// Parses CLI args, executes the command, prints the response, and returns an exit code.
pub fn run_from_env() -> i32 {
    init_tracing();
    let cli = Cli::parse();
    let command = cli.command.name();
    let human = cli.human;
    match execute(cli) {
        Ok(outcome) => match print_success(outcome, human) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{error}");
                1
            }
        },
        Err(error) => match print_failure(command, &error) {
            Ok(()) => 1,
            Err(print_error) => {
                eprintln!("{error}");
                eprintln!("{print_error}");
                1
            }
        },
    }
}

fn init_tracing() {
    match tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .try_init()
    {
        Ok(()) | Err(_) => {}
    }
}
