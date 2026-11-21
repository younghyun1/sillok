//! Temporary Git worktree for one sync attempt, driven through the `git` CLI.
//!
//! The user's own Git setup (SSH keys, credential helpers) handles auth.
//! Every command runs non-interactively: an agent cannot answer a password
//! prompt, so a prompt would hang the tool instead of failing.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::domain::id::ArchiveId;
use crate::error::SillokError;
use crate::sync::config::SyncConfig;

/// A throwaway clone; deleted on drop.
#[derive(Debug)]
pub struct Worktree {
    root: PathBuf,
    config: SyncConfig,
}

impl Worktree {
    /// Creates the worktree and checks out the remote branch when it exists.
    pub fn prepare(config: &SyncConfig) -> Result<Self, SillokError> {
        let root = std::env::temp_dir().join(format!("sillok-sync-{}", ArchiveId::new_v7()));
        if let Err(error) = std::fs::create_dir_all(&root) {
            return Err(error.into());
        }
        let tree = Self {
            root,
            config: config.clone(),
        };
        crate::logging::ensure();
        let branch = config.branch.as_str();
        let remote_ref = format!("refs/remotes/origin/{branch}");
        let steps: [&[&str]; 3] = [
            &["init", "-q"],
            &["remote", "add", "origin", config.url.as_str()],
            &["config", "commit.gpgsign", "false"],
        ];
        for args in steps {
            if let Err(error) = tree.git(args) {
                return Err(error);
            }
        }
        // Fetch first: when the branch exists, which is every sync after the
        // first, that is the only round trip needed before the push. Asking
        // `ls-remote` whether it exists beforehand doubled the connections.
        let refspec = format!("+refs/heads/{branch}:{remote_ref}");
        let fetched = match run(
            &tree.root,
            &["fetch", "-q", "--depth", "1", "origin", refspec.as_str()],
        ) {
            Ok(output) if output.status.success() => true,
            Ok(output) => match tree.branch_is_missing(branch, &failure(&output)) {
                Ok(()) => false,
                Err(error) => return Err(error),
            },
            Err(error) => return Err(error),
        };
        let checkout = match fetched {
            true => tree.git(&["checkout", "-q", "-B", branch, remote_ref.as_str()]),
            false => tree.git(&["checkout", "-q", "-B", branch]),
        };
        match checkout {
            Ok(_) => Ok(tree),
            Err(error) => Err(error),
        }
    }

    /// Decides whether a failed fetch only means the branch does not exist
    /// yet (an empty remote or a first sync), returning the fetch failure
    /// otherwise. Git's own wording settles the common case; anything else
    /// is confirmed with `ls-remote`, so an unfamiliar message costs one
    /// extra round trip instead of a wrong answer.
    fn branch_is_missing(&self, branch: &str, fetch_failure: &str) -> Result<(), SillokError> {
        if is_missing_ref(fetch_failure) {
            return Ok(());
        }
        match self.git(&["ls-remote", "--heads", "origin", branch]) {
            Ok(heads) if heads.trim().is_empty() => Ok(()),
            Ok(_) | Err(_) => Err(SillokError::sync(
                "sync_git_error",
                format!("git fetch failed: {fetch_failure}"),
            )),
        }
    }

    /// Worktree root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directory holding the layout.
    pub fn layout_dir(&self) -> PathBuf {
        self.root.join(&self.config.dir)
    }

    /// Stages the layout (and a removed legacy file), commits, and pushes.
    /// Returns the new commit, or `None` when nothing changed.
    pub fn commit_and_push(&self, message: &str) -> Result<Option<String>, SillokError> {
        if let Err(error) = self.git(&["add", "-A", "--", self.config.dir.as_str()]) {
            return Err(error);
        }
        // The 0.10 artifact is usually gone already (an earlier sync removed
        // it), and `git add` rejects a path that matches nothing; `rm
        // --cached --ignore-unmatch` stages its deletion only when tracked.
        if let Some(legacy) = &self.config.legacy_path
            && let Err(error) = self.git(&[
                "rm",
                "-q",
                "--cached",
                "--ignore-unmatch",
                "--",
                legacy.as_str(),
            ])
        {
            return Err(error);
        }
        let staged = match self.git(&["diff", "--cached", "--name-only"]) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        if staged.trim().is_empty() {
            return Ok(None);
        }
        let commit = [
            "-c",
            "user.name=sillok",
            "-c",
            "user.email=sillok@localhost",
            "commit",
            "-q",
            "-m",
            message,
        ];
        if let Err(error) = self.git(&commit) {
            return Err(error);
        }
        let head = match self.git(&["rev-parse", "HEAD"]) {
            Ok(value) => value.trim().to_string(),
            Err(error) => return Err(error),
        };
        let target = format!("HEAD:refs/heads/{}", self.config.branch);
        match run(&self.root, &["push", "-q", "origin", target.as_str()]) {
            Ok(output) if output.status.success() => Ok(Some(head)),
            Ok(output) => {
                let message = failure(&output);
                match is_rejection(&message) {
                    true => Err(SillokError::PushRejected(message)),
                    false => Err(SillokError::sync(
                        "sync_git_error",
                        format!("git push failed: {message}"),
                    )),
                }
            }
            Err(error) => Err(error),
        }
    }

    fn git(&self, args: &[&str]) -> Result<String, SillokError> {
        match run(&self.root, args) {
            Ok(output) if output.status.success() => {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            }
            Ok(output) => {
                let name = match args.first() {
                    Some(value) => *value,
                    None => "",
                };
                Err(SillokError::sync(
                    "sync_git_error",
                    format!("git {name} failed: {}", failure(&output)),
                ))
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            tracing::warn!(path = %self.root.display(), error = %error, "Failed to remove sync worktree");
        }
    }
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, SillokError> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        // Failures are classified by Git's English messages.
        .env("LC_ALL", "C");
    // Keep the user's SSH command if they set one; otherwise forbid prompts.
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        command.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    match command.output() {
        Ok(output) => Ok(output),
        Err(error) => Err(SillokError::sync(
            "sync_git_error",
            format!("could not run git: {error}"),
        )),
    }
}

/// Whether a push failed only because the remote moved, which a fresh
/// attempt can fix. Auth, hook, and network failures are not retryable.
fn is_rejection(stderr: &str) -> bool {
    [
        "[rejected]",
        "non-fast-forward",
        "fetch first",
        "stale info",
    ]
    .iter()
    .any(|marker| stderr.contains(marker))
}

/// Whether a fetch failed because the remote has no such branch.
fn is_missing_ref(stderr: &str) -> bool {
    stderr.contains("couldn't find remote ref")
}

fn failure(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .trim()
        .lines()
        .take(5)
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::{is_missing_ref, is_rejection};

    #[test]
    fn a_missing_branch_is_not_a_fetch_failure() {
        assert!(is_missing_ref(
            "fatal: couldn't find remote ref refs/heads/main"
        ));
        assert!(!is_missing_ref(
            "fatal: unable to access 'https://example.com/a.git/': Could not resolve host"
        ));
    }

    #[test]
    fn only_moved_remotes_are_rejections() {
        assert!(is_rejection(
            " ! [rejected]        HEAD -> main (fetch first) | error: failed to push some refs"
        ));
        assert!(!is_rejection(
            "remote: Permission to o/r.git denied to u. | fatal: unable to access"
        ));
        assert!(!is_rejection(
            "remote: error: hook declined to update refs/heads/main"
        ));
    }
}
