//! Shared helpers for CLI integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::cargo::CommandCargoExt;
use serde_json::Value;

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Builds an error for a failed expectation.
pub fn fail(message: impl Into<String>) -> Box<dyn std::error::Error> {
    Box::new(std::io::Error::other(message.into()))
}

/// A temporary directory with a store path inside it.
pub struct Sandbox {
    pub dir: tempfile::TempDir,
    pub store: PathBuf,
}

impl Sandbox {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let dir = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => return Err(Box::new(error)),
        };
        let store = dir.path().join("sillok.db");
        Ok(Self { dir, store })
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Runs sillok against this store.
    pub fn run(&self, args: &[&str]) -> Result<Output, Box<dyn std::error::Error>> {
        run_in(&self.store, self.dir.path(), args)
    }

    /// Runs and requires success; returns trimmed stdout.
    pub fn ok(&self, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let output = match self.run(args) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        if !output.status.success() {
            return Err(fail(format!(
                "`sillok {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Runs and parses stdout as JSON.
    pub fn json(&self, args: &[&str]) -> Result<Value, Box<dyn std::error::Error>> {
        match self.ok(args) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(value) => Ok(value),
                Err(error) => Err(fail(format!("bad JSON `{text}`: {error}"))),
            },
            Err(error) => Err(error),
        }
    }

    /// Runs, requires failure, and parses the stderr error.
    pub fn error(&self, args: &[&str]) -> Result<(i32, Value), Box<dyn std::error::Error>> {
        let output = match self.run(args) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        if output.status.success() {
            return Err(fail(format!(
                "`sillok {}` unexpectedly succeeded",
                args.join(" ")
            )));
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let last = match stderr.lines().last() {
            Some(line) => line.to_string(),
            None => return Err(fail("no stderr")),
        };
        let code = match output.status.code() {
            Some(value) => value,
            None => -1,
        };
        match serde_json::from_str(&last) {
            Ok(value) => Ok((code, value)),
            Err(error) => Err(fail(format!("bad error JSON `{last}`: {error}"))),
        }
    }
}

/// Runs the binary with a clean, deterministic environment.
pub fn run_in(
    store: &Path,
    cwd: &Path,
    args: &[&str],
) -> Result<Output, Box<dyn std::error::Error>> {
    let mut command = match std::process::Command::cargo_bin("sillok") {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    command
        .current_dir(cwd)
        .env("SILLOK_STORE", store)
        .env("SILLOK_TZ", "UTC")
        .env("TZ", "UTC")
        .env_remove("SILLOK_OUTPUT")
        .env_remove("SILLOK_SESSION")
        .env_remove("SILLOK_ACTOR")
        .args(args);
    match command.output() {
        Ok(value) => Ok(value),
        Err(error) => Err(Box::new(error)),
    }
}

/// Runs git and requires success.
pub fn git(cwd: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = match std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
    {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    if !output.status.success() {
        return Err(fail(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Path of a committed fixture.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/v0_10")
        .join(name)
}

/// Every string at `pointer` in each element of the array at `array`.
pub fn strings(value: &Value, array: &str, pointer: &str) -> Vec<String> {
    match value.pointer(array).and_then(Value::as_array) {
        Some(items) => items
            .iter()
            .filter_map(|item| {
                item.pointer(pointer)
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect(),
        None => Vec::new(),
    }
}
