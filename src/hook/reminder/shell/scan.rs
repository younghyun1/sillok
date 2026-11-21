//! Classify a shell script: did it change files or systems, and did it write to Sillok?

use super::{
    context::{Context, assignment},
    programs, tokenize,
};
use crate::hook::reminder::paths;
use std::path::Path;

/// `bash -c` and `eval` bodies nested deeper than this are not inspected.
const MAX_DEPTH: usize = 4;

/// The first change a script makes, if any, and whether it wrote a Sillok record.
#[derive(Debug, Default, PartialEq)]
pub struct Scan {
    pub change: Option<String>,
    pub record: bool,
}

/// Classify `script` as run from `cwd`.
pub fn scan(script: &str, cwd: Option<&Path>) -> Scan {
    let mut result = Scan::default();
    visit(script, &mut Context::new(cwd), 0, &mut result);
    result
}

/// Commands run in order, so each sees the directory and variables set before it;
/// a nested `bash -c` body starts from a copy, as a child shell would.
fn visit(script: &str, context: &mut Context, depth: usize, result: &mut Scan) {
    for simple in tokenize::split(script) {
        let words: Vec<String> = simple
            .words
            .iter()
            .map(|word| context.expand(word))
            .collect();
        if result.change.is_none() {
            for target in &simple.writes {
                if let Some(label) = paths::written(&context.expand(target), context.cwd.as_deref())
                {
                    result.change = Some(format!("write {label}"));
                    break;
                }
            }
        }
        context.observe(&words);
        let command = command_words(&words);
        let (program, args) = match command.split_first() {
            Some((program, args)) => (basename(program), args),
            None => continue,
        };
        match program {
            "bash" | "sh" | "zsh" | "fish" | "dash" | "ksh" => match inline_script(args) {
                Some(body) if depth < MAX_DEPTH => {
                    visit(body, &mut context.clone(), depth + 1, result)
                }
                Some(_) | None => (),
            },
            "eval" if depth < MAX_DEPTH => visit(&args.join(" "), context, depth + 1, result),
            "sillok" => {
                if records(args) {
                    result.record = true;
                }
            }
            _ => {
                if result.change.is_none() {
                    result.change = programs::mutation(program, args, context.cwd.as_deref());
                }
            }
        }
    }
}

/// Words left after leading assignments, shell keywords, and wrapper commands such
/// as `sudo`, `env`, `timeout`, `xargs`, and the `rtk` output filter.
fn command_words(words: &[String]) -> &[String] {
    let mut rest = words;
    loop {
        let (first, tail) = match rest.split_first() {
            Some(split) => split,
            None => return rest,
        };
        if assignment(first).is_some() || KEYWORDS.contains(&first.as_str()) {
            rest = tail;
            continue;
        }
        rest = match basename(first) {
            "sudo" | "doas" => skip_options(tail, &["-u", "-g", "-h", "-p", "-C", "-D", "-U"]),
            "command" | "builtin" | "exec" | "nohup" | "time" | "noglob" => skip_options(tail, &[]),
            "nice" => skip_options(tail, &["-n"]),
            "env" => skip_options(tail, &["-u", "-C", "-S", "--unset", "--chdir"]),
            "timeout" | "gtimeout" => {
                let after = skip_options(tail, &["-s", "-k", "--signal", "--kill-after"]);
                // The first operand is the duration, not the command.
                match after.split_first() {
                    Some((_, command)) => command,
                    None => after,
                }
            }
            "xargs" => skip_options(tail, &["-I", "-n", "-P", "-L", "-d", "-s", "-E", "-a"]),
            "rtk" => match tail.split_first() {
                Some((word, command)) if word == "proxy" => command,
                Some(_) | None => tail,
            },
            _ => return rest,
        };
    }
}

/// Keywords that can precede a command in sh or fish without being one.
const KEYWORDS: &[&str] = &[
    "!", "{", "}", "[[", "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "and",
    "or", "not", "begin", "end",
];

/// Drop leading options, and the value after each option in `with_value`.
pub fn skip_options<'a>(words: &'a [String], with_value: &[&str]) -> &'a [String] {
    let mut rest = words;
    while let Some((first, tail)) = rest.split_first() {
        if first == "--" {
            return tail;
        }
        if !first.starts_with('-') || first == "-" {
            break;
        }
        rest = match tail.split_first() {
            Some((_, after)) if with_value.contains(&first.as_str()) => after,
            Some(_) | None => tail,
        };
    }
    rest
}

/// Program name without its directory, so `/opt/homebrew/bin/gh` is `gh`.
pub fn basename(word: &str) -> &str {
    match word.rsplit(['/', '\\']).next() {
        Some(name) => name,
        None => word,
    }
}

/// The script given to a shell through `-c` or `--command`, as in `bash -lc '...'`.
fn inline_script(args: &[String]) -> Option<&str> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let command_flag = arg == "--command"
            || (arg.starts_with('-') && !arg.starts_with("--") && arg.contains('c'));
        if command_flag {
            return iter.next().map(String::as_str);
        }
    }
    None
}

/// Sillok subcommands that write the chronicle; reads such as `status` do not count.
fn records(args: &[String]) -> bool {
    match skip_options(args, &["--store", "--at", "--tz"]).split_first() {
        Some((sub, tail)) => match sub.as_str() {
            "note" | "amend" | "move" | "retract" | "restore" | "import" => true,
            "objective" => matches!(
                skip_options(tail, &[]).first().map(String::as_str),
                Some("add" | "complete")
            ),
            _ => false,
        },
        None => false,
    }
}
