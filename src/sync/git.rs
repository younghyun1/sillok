use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::archive_codec::{decode_archive, encode_sync_archive};
use crate::domain::archive::Archive;
use crate::domain::id::ChronicleId;
use crate::error::SillokError;
use crate::sync::config::SyncConfig;

pub const SYNC_COMMIT_MESSAGE: &str = "sync: update sillok archive";

/// Result of committing and pushing an artifact.
#[derive(Debug, Clone)]
pub struct PushOutcome {
    pub commit: Option<String>,
    pub pushed: bool,
}

/// Git worktree used for one sync operation.
#[derive(Debug)]
pub struct GitWorktree {
    root: PathBuf,
    config: SyncConfig,
}

impl GitWorktree {
    /// Creates a temporary worktree and checks out the configured branch if present.
    pub fn prepare(config: SyncConfig) -> Result<Self, SillokError> {
        config.validate()?;
        let root = temp_worktree_path();
        fs::create_dir_all(&root)?;
        let worktree = Self { root, config };
        worktree.git(["init"])?;
        worktree.git(["remote", "add", "origin", &worktree.config.url])?;
        if worktree.branch_exists()? {
            worktree.git([
                "fetch",
                "--depth",
                "1",
                "origin",
                &format!(
                    "refs/heads/{}:refs/remotes/origin/{}",
                    worktree.config.branch, worktree.config.branch
                ),
            ])?;
            worktree.git([
                "checkout",
                "-B",
                &worktree.config.branch,
                &format!("refs/remotes/origin/{}", worktree.config.branch),
            ])?;
        } else {
            worktree.git(["checkout", "-B", &worktree.config.branch])?;
        }
        worktree.git(["config", "user.name", "sillok"])?;
        worktree.git(["config", "user.email", "sillok@localhost"])?;
        Ok(worktree)
    }

    /// Reads the configured archive artifact if it exists.
    pub fn read_archive(&self) -> Result<Option<Archive>, SillokError> {
        let path = self.artifact_path();
        let bytes = match fs::read(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        Ok(Some(decode_archive(&bytes)?))
    }

    /// Writes, commits, and pushes an archive artifact.
    pub fn write_commit_push(&self, archive: &Archive) -> Result<PushOutcome, SillokError> {
        let path = self.artifact_path();
        match path.parent() {
            Some(parent) => fs::create_dir_all(parent)?,
            None => {
                return Err(SillokError::new(
                    "sync_git_error",
                    format!("artifact path `{}` has no parent", path.display()),
                ));
            }
        }
        let encoded = encode_sync_archive(archive)?;
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)?;
            file.write_all(&encoded)?;
            file.sync_all()?;
        }
        self.git(["add", "--", &self.config.path])?;
        let status = self.git(["status", "--porcelain", "--", &self.config.path])?;
        if status.stdout.trim().is_empty() {
            return Ok(PushOutcome {
                commit: self.current_head()?,
                pushed: false,
            });
        }
        self.git(["commit", "-m", SYNC_COMMIT_MESSAGE])?;
        let head = self.current_head()?;
        self.push_head()?;
        Ok(PushOutcome {
            commit: head,
            pushed: true,
        })
    }

    fn branch_exists(&self) -> Result<bool, SillokError> {
        let output = run_git(
            None,
            [
                "ls-remote",
                "--heads",
                &self.config.url,
                &self.config.branch,
            ],
        )?;
        Ok(!output.stdout.trim().is_empty())
    }

    fn artifact_path(&self) -> PathBuf {
        self.root.join(&self.config.path)
    }

    fn current_head(&self) -> Result<Option<String>, SillokError> {
        let output = raw_git(Some(&self.root), ["rev-parse", "HEAD"])?;
        if output.status.success() {
            let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if head.is_empty() {
                Ok(None)
            } else {
                Ok(Some(head))
            }
        } else {
            Ok(None)
        }
    }

    fn push_head(&self) -> Result<(), SillokError> {
        let output = raw_git(
            Some(&self.root),
            [
                "push",
                "origin",
                &format!("HEAD:refs/heads/{}", self.config.branch),
            ],
        )?;
        if output.status.success() {
            Ok(())
        } else {
            Err(SillokError::new(
                "sync_push_rejected",
                git_failure_message("git push", &output),
            ))
        }
    }

    fn git<I, S>(&self, args: I) -> Result<GitOutput, SillokError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        run_git(Some(&self.root), args)
    }
}

impl Drop for GitWorktree {
    fn drop(&mut self) {
        match fs::remove_dir_all(&self.root) {
            Ok(()) | Err(_) => {}
        }
    }
}

#[derive(Debug)]
struct GitOutput {
    stdout: String,
}

fn run_git<I, S>(cwd: Option<&Path>, args: I) -> Result<GitOutput, SillokError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = raw_git(cwd, args)?;
    if output.status.success() {
        Ok(GitOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        })
    } else {
        Err(SillokError::new(
            "sync_git_error",
            git_failure_message("git", &output),
        ))
    }
}

fn raw_git<I, S>(cwd: Option<&Path>, args: I) -> Result<Output, SillokError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut command = Command::new("git");
    if let Some(path) = cwd {
        command.current_dir(path);
    }
    for arg in args {
        command.arg(arg);
    }
    command.output().map_err(|error| {
        SillokError::new("sync_git_error", format!("failed to execute git: {error}"))
    })
}

fn git_failure_message(command: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format!("{command} failed: stdout={stdout} stderr={stderr}")
}

fn temp_worktree_path() -> PathBuf {
    std::env::temp_dir().join(format!("sillok-sync-{}", ChronicleId::new_v7()))
}
