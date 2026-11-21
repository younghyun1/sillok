//! Shell programs that write or remove files in place, such as `sed -i` and `mv`.
//! Targets under scratch space do not count; see `paths::written`.

use super::programs::positional;
use crate::hook::reminder::paths;
use std::path::Path;

/// The first non-scratch file `program` writes or removes, described for a reminder.
pub fn files(program: &str, args: &[String], cwd: Option<&Path>) -> Option<String> {
    let targets: Vec<&str> = match program {
        "sed" => in_place_targets(args, &["-e", "--expression", "-f", "--file", "-l"]),
        "perl" => in_place_targets(args, &["-e", "-E", "-I", "-M"]),
        // `sd FIND REPLACE [FILES]` rewrites files in place; without files it filters stdin.
        "sd" => positional(args, &["-f", "--flags", "-n", "--max-replacements"])
            .into_iter()
            .skip(2)
            .collect(),
        "tee" => positional(args, &[]),
        // Copies and moves write only their destination.
        "cp" | "mv" | "install" | "ln" | "rsync" => {
            let words = positional(args, &["-t", "--target-directory", "-m", "--mode", "-e"]);
            match words.as_slice() {
                [_, .., destination] => vec![*destination],
                _ => Vec::new(),
            }
        }
        "rm" | "rmdir" | "unlink" => positional(args, &[]),
        "truncate" => positional(args, &["-s", "--size", "-r", "--reference"]),
        _ => Vec::new(),
    };
    for target in targets {
        if let Some(label) = paths::written(target, cwd) {
            return Some(format!("{program} {label}"));
        }
    }
    None
}

/// Files edited by `sed -i` or `perl -i`; without an in-place flag nothing is written.
/// When no script option is given, the first operand is the script, not a file.
fn in_place_targets<'a>(args: &'a [String], script_options: &[&str]) -> Vec<&'a str> {
    let in_place = args.iter().any(|arg| {
        arg == "--in-place"
            || arg.starts_with("--in-place=")
            || (arg.starts_with('-') && !arg.starts_with("--") && arg.contains('i'))
    });
    if !in_place {
        return Vec::new();
    }
    // BSD sed spells "no backup" as `-i ''`, which leaves an empty operand.
    let operands: Vec<&str> = positional(args, script_options)
        .into_iter()
        .filter(|operand| !operand.is_empty())
        .collect();
    let scripted = args
        .iter()
        .any(|arg| script_options.contains(&arg.as_str()));
    if scripted {
        operands
    } else {
        operands.into_iter().skip(1).collect()
    }
}
