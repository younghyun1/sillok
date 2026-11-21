//! Per-program rules for commands that change remote systems or files.
//!
//! The tables cover the control planes this policy bundle names (git, GitHub,
//! Google Cloud, Kubernetes, OpenTofu, GAM, 1Password, AWS, Azure, PostgreSQL)
//! plus in-place file writers.

use super::scan::skip_options;
use super::tables::{
    AWS_PREFIXES, AWS_S3, AZ, BQ, CARGO, GAM, GCLOUD, GH, GIT, GSUTIL, GSUTIL_POLICY, HELM,
    KUBECTL, KUBECTL_ROLLOUT, KUBECTL_VALUES, NPM, OP, SQL_WRITES, TOFU, TOFU_STATE,
};
use std::path::Path;

/// A short description of the change `program` makes with `args`, if it makes one.
pub fn mutation(program: &str, args: &[String], cwd: Option<&Path>) -> Option<String> {
    match program {
        "git" => first(
            program,
            &positional(
                args,
                &["-C", "-c", "--git-dir", "--work-tree", "--namespace"],
            ),
            GIT,
        ),
        "gh" => gh(args),
        "gcloud" => verb(program, &leading(args), GCLOUD),
        "gsutil" => gsutil(args),
        "bq" => bq(args),
        "kubectl" => kubectl(args),
        "helm" => first(
            program,
            &positional(
                args,
                &["-n", "--namespace", "--kube-context", "--kubeconfig"],
            ),
            HELM,
        ),
        "tofu" | "terraform" => tofu(program, args),
        "gam" => verb(program, &leading(args), GAM),
        "op" => verb(program, &leading(args), OP),
        "aws" => aws(args),
        "az" => verb(program, &leading(args), AZ),
        "psql" | "connect-db" => sql_option(program, args),
        "curl" => curl(args),
        "cargo" => first(program, &positional(args, &[]), CARGO),
        "npm" | "pnpm" | "yarn" | "bun" => first(program, &positional(args, &[]), NPM),
        "docker" | "podman" => first(program, &positional(args, &[]), &["push"]),
        _ => super::writers::files(program, args, cwd),
    }
}

/// Words before the first option: the command path of most CLIs.
fn leading(args: &[String]) -> Vec<&str> {
    args.iter()
        .take_while(|arg| !arg.starts_with('-'))
        .take(8)
        .map(String::as_str)
        .collect()
}

/// Non-option words, skipping the value after each option in `with_value`.
pub fn positional<'a>(args: &'a [String], with_value: &[&str]) -> Vec<&'a str> {
    let mut words = Vec::new();
    let mut rest = args;
    while !rest.is_empty() {
        rest = skip_options(rest, with_value);
        match rest.split_first() {
            Some((word, tail)) => {
                words.push(word.as_str());
                rest = tail;
            }
            None => break,
        }
    }
    words
}

fn first(program: &str, words: &[&str], set: &[&str]) -> Option<String> {
    match words.first() {
        Some(word) if set.contains(word) => Some(format!("{program} {word}")),
        Some(_) | None => None,
    }
}

/// The first listed verb among `words`, described with the command path up to it.
fn verb(program: &str, words: &[&str], verbs: &[&str]) -> Option<String> {
    match words.iter().position(|word| verbs.contains(word)) {
        Some(index) => words
            .get(..=index)
            .map(|path| format!("{program} {}", path.join(" "))),
        None => None,
    }
}

fn gh(args: &[String]) -> Option<String> {
    let words = positional(args, &["-R", "--repo"]);
    match words.as_slice() {
        ["api", rest @ ..] => {
            let graphql = rest.first() == Some(&"graphql");
            if api_writes(args, graphql) {
                Some("gh api".to_owned())
            } else {
                None
            }
        }
        [group, action, ..] => match GH.iter().find(|(name, _)| name == group) {
            Some((_, actions)) if actions.contains(action) => Some(format!("gh {group} {action}")),
            Some(_) | None => None,
        },
        _ => None,
    }
}

/// REST calls write with a non-GET method or with fields, which imply POST; GraphQL
/// always posts, so only a `mutation` document counts.
fn api_writes(args: &[String], graphql: bool) -> bool {
    if graphql {
        return args.iter().any(|arg| arg.contains("mutation"));
    }
    let mut fields = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let method = match arg.as_str() {
            "-X" | "--method" => iter.next().map(String::as_str),
            other => match (other.strip_prefix("--method="), other.strip_prefix("-X")) {
                (Some(method), _) => Some(method),
                (None, Some(method)) if !method.is_empty() => Some(method),
                (None, Some(_) | None) => None,
            },
        };
        if let Some(method) = method {
            return !method.eq_ignore_ascii_case("GET") && !method.eq_ignore_ascii_case("HEAD");
        }
        let field = ["-f", "-F", "--field", "--raw-field", "--input"].contains(&arg.as_str())
            || arg.starts_with("--field=")
            || arg.starts_with("--raw-field=");
        fields |= field;
    }
    fields
}

fn gsutil(args: &[String]) -> Option<String> {
    let words = positional(args, &["-o", "-h", "-u", "-p"]);
    match words.as_slice() {
        [sub, ..] if GSUTIL.contains(sub) => Some(format!("gsutil {sub}")),
        [sub, next, ..] if GSUTIL_POLICY.contains(sub) && *next != "get" => {
            Some(format!("gsutil {sub} {next}"))
        }
        [sub @ ("cp" | "rsync"), .., last] if last.starts_with("gs://") => {
            Some(format!("gsutil {sub}"))
        }
        _ => None,
    }
}

fn bq(args: &[String]) -> Option<String> {
    let words = positional(
        args,
        &["--project_id", "--location", "--format", "--dataset_id"],
    );
    match words.as_slice() {
        [sub, ..] if BQ.contains(sub) => Some(format!("bq {sub}")),
        ["query", rest @ ..] if rest.iter().any(|query| sql_writes(query)) => {
            Some("bq query".to_owned())
        }
        _ => None,
    }
}

fn kubectl(args: &[String]) -> Option<String> {
    match positional(args, KUBECTL_VALUES).as_slice() {
        ["rollout", action, ..] if KUBECTL_ROLLOUT.contains(action) => {
            Some(format!("kubectl rollout {action}"))
        }
        [sub, ..] if KUBECTL.contains(sub) => Some(format!("kubectl {sub}")),
        _ => None,
    }
}

fn tofu(program: &str, args: &[String]) -> Option<String> {
    match positional(args, &[]).as_slice() {
        ["state", action, ..] if TOFU_STATE.contains(action) => {
            Some(format!("{program} state {action}"))
        }
        [sub, ..] if TOFU.contains(sub) => Some(format!("{program} {sub}")),
        _ => None,
    }
}

fn aws(args: &[String]) -> Option<String> {
    let words = positional(
        args,
        &[
            "--profile",
            "--region",
            "--output",
            "--endpoint-url",
            "--query",
        ],
    );
    match words.as_slice() {
        ["s3", op, ..] if AWS_S3.contains(op) => Some(format!("aws s3 {op}")),
        ["s3", op @ ("cp" | "sync"), .., last] if last.starts_with("s3://") => {
            Some(format!("aws s3 {op}"))
        }
        [service, op, ..] if AWS_PREFIXES.iter().any(|prefix| op.starts_with(prefix)) => {
            Some(format!("aws {service} {op}"))
        }
        _ => None,
    }
}

/// `psql -c` text that writes rows or schema.
fn sql_option(program: &str, args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let text = match arg.as_str() {
            "-c" | "--command" => iter.next().map(String::as_str),
            other => other.strip_prefix("--command="),
        };
        match text {
            Some(text) if sql_writes(text) => return Some(format!("{program} write")),
            Some(_) | None => (),
        }
    }
    None
}

/// Whole-word, case-insensitive SQL write keywords, so `updated_at` is not `update`.
pub fn sql_writes(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| {
            SQL_WRITES
                .iter()
                .any(|keyword| word.eq_ignore_ascii_case(keyword))
        })
}

/// Requests with a body or a non-GET method.
fn curl(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let method = match arg.as_str() {
            "-X" | "--request" => iter.next().map(String::as_str),
            other => match (other.strip_prefix("--request="), other.strip_prefix("-X")) {
                (Some(method), _) => Some(method),
                (None, Some(method)) if !method.is_empty() => Some(method),
                (None, Some(_) | None) => None,
            },
        };
        let writes = match method {
            Some(method) => {
                !method.eq_ignore_ascii_case("GET") && !method.eq_ignore_ascii_case("HEAD")
            }
            None => {
                ["-d", "-F", "-T", "--json", "--form", "--upload-file"].contains(&arg.as_str())
                    || arg.starts_with("--data")
                    || arg.starts_with("--form=")
                    || arg.starts_with("--json=")
            }
        };
        if writes {
            return Some("curl write".to_owned());
        }
    }
    None
}
