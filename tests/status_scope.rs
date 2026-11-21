//! `status` finds a repository's work from any worktree or clone, and the
//! store upgrades its additive schema in place.

mod support;

use std::path::Path;

use support::{Sandbox, TestResult, fail, git, run_in, strings};

/// Runs sillok in `cwd` against the sandbox store and parses stdout as JSON.
fn json_in(
    sandbox: &Sandbox,
    cwd: &Path,
    args: &[&str],
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let output = match run_in(&sandbox.store, cwd, args) {
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
    match serde_json::from_slice(&output.stdout) {
        Ok(value) => Ok(value),
        Err(error) => Err(fail(format!("bad JSON: {error}"))),
    }
}

fn repository(path: &Path, remote: Option<&str>) -> TestResult {
    if let Err(error) = std::fs::create_dir_all(path) {
        return Err(Box::new(error));
    }
    let steps: [&[&str]; 2] = [
        &["init", "-q", "-b", "main"],
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ],
    ];
    for args in steps {
        if let Err(error) = git(path, args) {
            return Err(error);
        }
    }
    match remote {
        Some(url) => match git(path, &["remote", "add", "origin", url]) {
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        },
        None => Ok(()),
    }
}

#[test]
fn a_linked_worktree_sees_the_main_checkout() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let main = sandbox.path("main");
    let linked = sandbox.path("linked");
    if let Err(error) = repository(&main, None) {
        return Err(error);
    }
    let linked_text = linked.display().to_string();
    if let Err(error) = git(
        &main,
        &["worktree", "add", "-q", &linked_text, "-b", "side"],
    ) {
        return Err(error);
    }
    if let Err(error) = run_in(
        &sandbox.store,
        &main,
        &["objective", "add", "Started in main"],
    ) {
        return Err(error);
    }
    let status = match json_in(&sandbox, &linked, &["status"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(
        strings(&status, "/objectives", "/text"),
        ["Started in main"]
    );
    assert_eq!(strings(&status, "/recent", "/text"), ["Started in main"]);
    assert!(status.get("remote").is_none());
    Ok(())
}

#[test]
fn clones_of_one_remote_share_status_and_others_do_not() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let first = sandbox.path("first");
    let second = sandbox.path("second");
    let other = sandbox.path("other");
    for (path, url) in [
        (&first, "https://user:secret@example.com/team/project.git"),
        (&second, "https://example.com/team/project.git"),
        (&other, "https://example.com/team/unrelated.git"),
    ] {
        if let Err(error) = repository(path, Some(url)) {
            return Err(error);
        }
    }
    if let Err(error) = run_in(
        &sandbox.store,
        &first,
        &["note", "Recorded in the first clone"],
    ) {
        return Err(error);
    }
    let (same, different) = match (
        json_in(&sandbox, &second, &["status"]),
        json_in(&sandbox, &other, &["status"]),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    assert_eq!(
        strings(&same, "/recent", "/text"),
        ["Recorded in the first clone"]
    );
    // Credentials in a remote URL never reach output.
    assert_eq!(same["remote"], "https://example.com/team/project.git");
    assert!(strings(&different, "/recent", "/text").is_empty());
    Ok(())
}

#[test]
fn a_store_from_an_earlier_release_gains_the_new_index() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    if let Err(error) = sandbox.ok(&["note", "before the upgrade"]) {
        return Err(error);
    }
    // Put the store back into the shape 1.0 wrote.
    let conn = match rusqlite::Connection::open(&sandbox.store) {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    if let Err(error) = conn.execute_batch(
        "DROP INDEX work_context_git_remote_idx;
         DELETE FROM store_meta WHERE store_meta_key = 'schema_rev';",
    ) {
        return Err(Box::new(error));
    }
    // A read is enough to bring it forward, and it stays valid.
    let doctor = match sandbox.json(&["doctor"]) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    assert_eq!(doctor["valid"], true);
    let (index, revision) = match conn.query_row(
        "SELECT (SELECT COUNT(*) FROM sqlite_master WHERE name = 'work_context_git_remote_idx'),
                (SELECT store_meta_value FROM store_meta WHERE store_meta_key = 'schema_rev')",
        [],
        |row| Ok((row.get::<_, i64>(0), row.get::<_, String>(1))),
    ) {
        Ok((Ok(index), Ok(revision))) => (index, revision),
        Ok(_) => return Err(fail("unexpected column types")),
        Err(error) => return Err(Box::new(error)),
    };
    assert_eq!(index, 1);
    assert_eq!(revision, "1");
    Ok(())
}

#[test]
fn an_empty_store_variable_means_the_default_store() -> TestResult {
    let sandbox = match Sandbox::new() {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let mut command = match assert_cmd::cargo::CommandCargoExt::cargo_bin("sillok") {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    let command: &mut std::process::Command = &mut command;
    let output = match command
        .current_dir(sandbox.dir.path())
        .env("SILLOK_STORE", "")
        .env("HOME", sandbox.dir.path())
        .env("XDG_DATA_HOME", sandbox.path("data"))
        .env("SILLOK_TZ", "UTC")
        .args(["objective", "list"])
        .output()
    {
        Ok(value) => value,
        Err(error) => return Err(Box::new(error)),
    };
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "{\"records\":[]}"
    );
    Ok(())
}
