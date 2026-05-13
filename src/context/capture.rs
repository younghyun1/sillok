use std::path::Path;
use std::process::Command;

use tracing::warn;

use crate::domain::event::WorkContext;

/// Captures cwd and git metadata without making logging dependent on git.
pub fn capture() -> (WorkContext, Vec<String>) {
    let mut warnings = Vec::new();
    let cwd = match std::env::current_dir() {
        Ok(path) => Some(path.display().to_string()),
        Err(error) => {
            warnings.push(format!("could not read current directory: {error}"));
            warn!(error = %error, "Failed to read current directory");
            None
        }
    };

    let cwd_path = cwd.as_ref().map(Path::new);
    let git_root = git_value(cwd_path, &["rev-parse", "--show-toplevel"]);
    let git_branch = git_value(cwd_path, &["branch", "--show-current"]);
    let git_head = git_value(cwd_path, &["rev-parse", "HEAD"]);
    let git_remote = git_value(cwd_path, &["config", "--get", "remote.origin.url"]);

    (
        WorkContext {
            cwd,
            git_root,
            git_branch,
            git_head,
            git_remote,
        },
        warnings,
    )
}

fn git_value(cwd: Option<&Path>, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(path) = cwd {
        command.current_dir(path);
    }
    match command.output() {
        Ok(output) if output.status.success() => {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if value.is_empty() { None } else { Some(value) }
        }
        Ok(_) | Err(_) => None,
    }
}
