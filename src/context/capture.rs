//! Captures where an event was recorded.
//!
//! Writes only: reads never need the context. The two Git queries run as
//! parallel child processes, so capture costs one process round trip rather
//! than four sequential ones as in 0.10.

use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::domain::event::context::{Scope, WorkContext, sanitize_remote};

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

/// Scope for `status`: the repository around the working directory, found
/// with two parallel Git queries, else the directory itself.
pub fn current_scope() -> Option<Scope> {
    let cwd = match std::env::current_dir() {
        Ok(path) => path,
        Err(_) => return None,
    };
    let dir = Some(cwd.as_path());
    let rev = spawn(dir, &["rev-parse", "--show-toplevel", "--git-common-dir"]);
    let remote = spawn(dir, &["config", "--get", "remote.origin.url"]);
    let rev = finish(rev);
    // Always reaped, even when the directory turns out not to be a repository.
    let remote = finish(remote).map(|url| sanitize_remote(&url));
    let mut lines = match &rev {
        Some(output) => output.lines(),
        None => "".lines(),
    };
    let root = non_empty(lines.next().map(str::to_string));
    let common = lines.next();
    Some(match root {
        Some(root) => Scope {
            main_root: match common {
                Some(value) => main_checkout(value, &root),
                None => None,
            },
            root,
            remote,
        },
        None => Scope {
            root: cwd.display().to_string(),
            main_root: None,
            remote: None,
        },
    })
}

/// Root of the main checkout, given the common Git directory of a linked
/// worktree. Git prints that directory as an absolute path ending in `.git`
/// from a linked worktree and as a relative one from the main checkout.
fn main_checkout(common_dir: &str, root: &str) -> Option<String> {
    let path = Path::new(common_dir.trim());
    if !path.is_absolute() || path.file_name() != Some(std::ffi::OsStr::new(".git")) {
        return None;
    }
    match path.parent() {
        Some(parent) if parent != Path::new(root) => Some(parent.display().to_string()),
        Some(_) | None => None,
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

#[cfg(test)]
mod tests {
    use super::main_checkout;

    #[test]
    fn only_linked_worktrees_have_a_main_checkout() {
        assert_eq!(
            main_checkout("/work/repo/.git", "/work/repo-wt"),
            Some("/work/repo".to_string())
        );
        // The main checkout itself, a relative path, and a submodule's Git directory.
        assert_eq!(main_checkout("/work/repo/.git", "/work/repo"), None);
        assert_eq!(main_checkout(".git", "/work/repo"), None);
        assert_eq!(
            main_checkout("/work/repo/.git/modules/x", "/work/repo/x"),
            None
        );
    }
}
