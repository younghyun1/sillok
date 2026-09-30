//! Process entry: logging, argument parsing, execution, and exit codes.

use clap::Parser;
use clap::error::ErrorKind;
use tracing_subscriber::EnvFilter;

use crate::cli::args::root::Cli;
use crate::cli::output::outcome::Mode;
use crate::cli::output::render::{print_failure, print_success, write_stdout};
use crate::commands::dispatch::execute;
use crate::error::SillokError;

/// Environment variable holding a `tracing` filter; defaults to
/// `sillok=info,warn` so only notable events (such as a migration) log.
pub const LOG_ENV: &str = "SILLOK_LOG";

/// Runs the CLI and returns the process exit code.
pub fn run_from_env() -> i32 {
    init_tracing();
    let cli = match Cli::try_parse() {
        Ok(value) => value,
        Err(error) => return usage(error),
    };
    let command = cli.command.name();
    let mode = Mode::resolve(cli.json, cli.human);
    match execute(cli) {
        Ok(outcome) => match print_success(outcome, mode) {
            Ok(()) => 0,
            Err(error) => {
                print_failure(command, &error, mode);
                error.exit_code()
            }
        },
        Err(error) => {
            print_failure(command, &error, mode);
            error.exit_code()
        }
    }
}

/// Help and version print normally; real usage errors become JSON on stderr
/// with exit code 2 so agents can tell them from runtime failures.
fn usage(error: clap::Error) -> i32 {
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            match write_stdout(error.to_string().trim_end()) {
                Ok(()) => 0,
                Err(_) => 1,
            }
        }
        // `sillok objective` without a subcommand: help goes to stderr so a
        // captured `$(...)` never mistakes it for output, and the exit is 2.
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            eprintln!("{}", error.to_string().trim_end());
            let failure = SillokError::Usage("missing subcommand or argument".to_string());
            print_failure("usage", &failure, Mode::resolve(false, false));
            failure.exit_code()
        }
        _ => {
            let message = error.to_string();
            let first = match message.lines().next() {
                Some(line) => line.trim_start_matches("error: ").to_string(),
                None => message.clone(),
            };
            let failure = SillokError::Usage(first);
            let mode = Mode::resolve(false, false);
            print_failure("usage", &failure, mode);
            failure.exit_code()
        }
    }
}

fn init_tracing() {
    let filter = match EnvFilter::try_from_env(LOG_ENV) {
        Ok(value) => value,
        Err(_) => EnvFilter::new("sillok=info,warn"),
    };
    // Logs go to stderr as JSON; stdout carries command output only.
    match tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_current_span(true)
        .with_span_list(false)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .try_init()
    {
        Ok(()) | Err(_) => {}
    }
}
