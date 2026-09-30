//! Many agents writing and reading one store at once.

mod support;

use std::sync::Arc;
use std::thread;

use support::{Sandbox, TestResult, fail, run_in};

const WRITERS: usize = 32;
const READERS: usize = 16;

#[test]
fn parallel_writers_and_readers_all_succeed() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => Arc::new(value),
        Err(error) => return Err(error),
    };
    if let Err(error) = sandbox.ok(&["init"]) {
        return Err(error);
    }
    let mut handles = Vec::with_capacity(WRITERS + READERS);
    for index in 0..WRITERS + READERS {
        let shared = Arc::clone(&sandbox);
        handles.push(thread::spawn(move || {
            let text = format!("parallel note {index}");
            let args: Vec<&str> = match index < WRITERS {
                true => vec!["note", text.as_str()],
                false => vec!["day"],
            };
            match run_in(&shared.store, shared.dir.path(), &args) {
                Ok(output) if output.status.success() => Ok(()),
                Ok(output) => Err(String::from_utf8_lossy(&output.stderr).to_string()),
                Err(error) => Err(error.to_string()),
            }
        }));
    }
    let mut failures = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(message)) => failures.push(message),
            Err(_) => failures.push("thread panicked".to_string()),
        }
    }
    if !failures.is_empty() {
        return Err(fail(format!(
            "{} commands failed: {:?}",
            failures.len(),
            failures
        )));
    }
    let found = match sandbox.json(&["query", "--text", "parallel note"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(found["records"].as_array().map(Vec::len), Some(WRITERS));
    let doctor = match sandbox.json(&["doctor"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(doctor["valid"], true);
    Ok(())
}
