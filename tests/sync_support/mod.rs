use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use assert_cmd::Command as AssertCommand;
use serde_json::Value;

pub fn boxed_error(message: String) -> Box<dyn std::error::Error> {
    Box::new(std::io::Error::other(message))
}

pub fn temp_store() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let dir = match tempfile::tempdir() {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    let store = dir.path().join("sillok.db");
    Ok((dir, store))
}

pub fn run_json(store: &Path, args: &[&str]) -> Result<Value, Box<dyn std::error::Error>> {
    let stdout = run_stdout(store, args)?;
    parse_json(&stdout)
}

pub fn run_failure_json(store: &Path, args: &[&str]) -> Result<Value, Box<dyn std::error::Error>> {
    let mut command = match AssertCommand::cargo_bin("sillok") {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    command.arg("--store").arg(store);
    command.env("TZ", "UTC");
    for arg in args {
        command.arg(arg);
    }
    let output = match command.output() {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    if output.status.success() {
        return Err(boxed_error("command unexpectedly succeeded".to_string()));
    }
    let stdout = match String::from_utf8(output.stdout) {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    parse_json(&stdout)
}

pub fn init_bare_remote(dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let remote = dir.join("remote.git");
    let remote_arg = remote.display().to_string();
    run_git(None, &["init", "--bare", &remote_arg])?;
    Ok(remote)
}

pub fn clone_remote(remote: &Path, target: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let remote_arg = remote.display().to_string();
    let target_arg = target.display().to_string();
    run_git(
        None,
        &["clone", "--branch", "main", &remote_arg, &target_arg],
    )?;
    Ok(())
}

pub fn remote_set(store: &Path, remote: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    let remote_arg = remote.display().to_string();
    run_json(store, &["sync", "remote", "set", &remote_arg])
}

fn run_stdout(store: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let mut command = match AssertCommand::cargo_bin("sillok") {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    command.arg("--store").arg(store);
    command.env("TZ", "UTC");
    for arg in args {
        command.arg(arg);
    }
    let output = match command.output() {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(boxed_error(format!(
            "command failed: stdout={stdout} stderr={stderr}"
        )));
    }
    match String::from_utf8(output.stdout) {
        Ok(value) => Ok(value),
        Err(error) => Err(Box::new(error)),
    }
}

fn parse_json(stdout: &str) -> Result<Value, Box<dyn std::error::Error>> {
    match serde_json::from_str::<Value>(stdout) {
        Ok(value) => Ok(value),
        Err(error) => Err(boxed_error(format!(
            "json parse failed: {error}; stdout={stdout}"
        ))),
    }
}

fn run_git(cwd: Option<&Path>, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let mut command = ProcessCommand::new("git");
    if let Some(path) = cwd {
        command.current_dir(path);
    }
    for arg in args {
        command.arg(arg);
    }
    let output = match command.output() {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    if output.status.success() {
        match String::from_utf8(output.stdout) {
            Ok(value) => Ok(value),
            Err(error) => Err(Box::new(error)),
        }
    } else {
        Err(boxed_error(format!(
            "git failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}
