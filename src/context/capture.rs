//! Captures where an event was recorded.
//!
//! Writes only: reads never need the context. The two Git queries run as
//! parallel child processes, so capture costs one process round trip rather
//! than four sequential ones as in 0.10.

use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::domain::event::context::{WorkContext, sanitize_remote};

/// Environment variable naming the agent session, recorded on each event.
pub const SESSION_ENV: &str = "SILLOK_SESSION";

/// Captures cwd, Git root/branch/head/remote, and the session label.
pub fn capture() -> (WorkContext, Vec<String>) {
    let mut warnings = Vec::new();
    let cwd = match std::env::current_dir() {
        Ok(path) => Some(path.display().to_string()),
        Err(error) => {
            warnings.push(format!("could not read current directory: {error}"));
            None
        }
    };
    let dir = cwd.as_deref().map(Path::new);
    // `--abbrev-ref` applies to every rev after it, so the full HEAD comes first.
    let rev = spawn(
        dir,
        &[
            "rev-parse",
            "--show-toplevel",
            "HEAD",
            "--abbrev-ref",
            "HEAD",
        ],
    );
    let remote = spawn(dir, &["config", "--get", "remote.origin.url"]);
    let (git_root, git_branch, git_head) = match finish(rev) {
        Some(output) => {
            let mut lines = output.lines().map(str::to_string);
            let root = non_empty(lines.next());
            let head = non_empty(lines.next());
            let branch = match non_empty(lines.next()) {
                // A detached HEAD reports the literal "HEAD" as its name.
                Some(name) if name == "HEAD" => None,
                other => other,
            };
            (root, branch, head)
        }
        // No commits yet: HEAD cannot be resolved, but root and branch can.
        None => (
            finish(spawn(dir, &["rev-parse", "--show-toplevel"])),
            finish(spawn(dir, &["branch", "--show-current"])),
            None,
        ),
    };
    let git_remote = finish(remote).map(|url| sanitize_remote(&url));
    let session = match std::env::var(SESSION_ENV) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Ok(_) | Err(_) => None,
    };
    (
        WorkContext {
            cwd,
            git_root,
            git_branch,
            git_head,
            git_remote,
            session,
        },
        warnings,
    )
}

/// Grouping key for `status`: the repository root, else the working directory.
pub fn current_key() -> Option<String> {
    let cwd = match std::env::current_dir() {
        Ok(path) => path.display().to_string(),
        Err(_) => return None,
    };
    match finish(spawn(
        Some(Path::new(&cwd)),
        &["rev-parse", "--show-toplevel"],
    )) {
        Some(root) => Some(root),
        None => Some(cwd),
    }
}

fn spawn(dir: Option<&Path>, args: &[&str]) -> Option<Child> {
    let mut command = Command::new("git");
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(path) = dir {
        command.current_dir(path);
    }
    match command.spawn() {
        Ok(child) => Some(child),
        Err(_) => None,
    }
}

/// Waits for a child and returns trimmed stdout when it succeeded.
fn finish(child: Option<Child>) -> Option<String> {
    let child = match child {
        Some(value) => value,
        None => return None,
    };
    match child.wait_with_output() {
        Ok(output) if output.status.success() => non_empty(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        )),
        Ok(_) | Err(_) => None,
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    match value {
        Some(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Some(_) | None => None,
    }
}
