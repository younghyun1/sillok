//! `skill` commands: install, inspect, and print the embedded agent skill.
//!
//! None of them touch the store, so they run without resolving one.

use serde_json::{Value, json};

use crate::cli::args::skill::{SkillCommand, SkillInstallArgs};
use crate::cli::output::outcome::Outcome;
use crate::error::SillokError;
use crate::skill::content::{self, VERSION, Version};
use crate::skill::install;
use crate::skill::roots::{Homes, Target, dir_target};

/// Dispatches `skill`.
pub fn dispatch(command: SkillCommand) -> Result<Outcome, SillokError> {
    match command {
        SkillCommand::Install(args) => run_install(args),
        SkillCommand::Status => status(),
        SkillCommand::Print => Ok(Outcome::text(
            "skill",
            content::render().trim_end().to_string(),
        )),
    }
}

/// `skill install`: explicit targets when given, else every tool found.
fn run_install(args: SkillInstallArgs) -> Result<Outcome, SillokError> {
    let explicit = !args.tools.is_empty() || !args.dirs.is_empty();
    let mut targets: Vec<Target> = Vec::new();
    if !explicit || !args.tools.is_empty() {
        let homes = match Homes::from_env() {
            Some(value) => value,
            None => {
                return Err(SillokError::invalid(
                    "skill_no_target",
                    "could not resolve a home directory; pass --dir <skill root>",
                ));
            }
        };
        match explicit {
            true => {
                for tool in &args.tools {
                    let target = homes.target(tool.tool());
                    if !targets.contains(&target) {
                        targets.push(target);
                    }
                }
            }
            false => targets = homes.detected(),
        }
    }
    for dir in &args.dirs {
        let target = dir_target(dir);
        if !targets.iter().any(|known| known.path == target.path) {
            targets.push(target);
        }
    }
    if targets.is_empty() {
        return Err(SillokError::invalid(
            "skill_no_target",
            "no Claude Code or Codex home found; pass --tool or --dir <skill root>",
        ));
    }
    let mut rows: Vec<Value> = Vec::with_capacity(targets.len());
    let mut lines: Vec<String> = Vec::with_capacity(targets.len());
    let mut warnings = Vec::new();
    for target in &targets {
        let action = match install::install(&target.path) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        lines.push(format!(
            "{} {} ({})",
            action.label(),
            target.path.display(),
            target.tool
        ));
        rows.push(json!({
            "tool": target.tool,
            "path": target.path.display().to_string(),
            "action": action.label(),
        }));
        duplicate_warning(target, &mut warnings);
    }
    Ok(
        Outcome::read("skill", json!({ "version": VERSION, "targets": rows }))
            .with_human(lines.join("\n"))
            .with_warnings(warnings),
    )
}

/// `skill status`: the copy each tool on this machine would load.
fn status() -> Result<Outcome, SillokError> {
    let targets = match Homes::from_env() {
        Some(homes) => homes.detected(),
        None => Vec::new(),
    };
    let mut rows: Vec<Value> = Vec::with_capacity(targets.len());
    let mut lines: Vec<String> = Vec::with_capacity(targets.len());
    let mut warnings = Vec::new();
    for target in &targets {
        let (state, stamped) = match install::state(&target.path) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let mut row = json!({
            "tool": target.tool,
            "path": target.path.display().to_string(),
            "state": state.label(),
        });
        if let (Some(version), Value::Object(map)) = (stamped, &mut row) {
            map.insert("installed_version".to_string(), json!(dotted(version)));
        }
        lines.push(format!(
            "{} {} ({})",
            state.label(),
            target.path.display(),
            target.tool
        ));
        rows.push(row);
        duplicate_warning(target, &mut warnings);
    }
    if lines.is_empty() {
        lines.push("no Claude Code or Codex home found".to_string());
    }
    Ok(
        Outcome::read("skill", json!({ "version": VERSION, "targets": rows }))
            .with_human(lines.join("\n"))
            .with_warnings(warnings),
    )
}

fn duplicate_warning(target: &Target, warnings: &mut Vec<String>) {
    if let Some(duplicate) = &target.duplicate {
        warnings.push(format!(
            "{} also discovers a second copy at {}; remove it so only {} is loaded",
            target.tool,
            duplicate.display(),
            target.path.display()
        ));
    }
}

fn dotted(version: Version) -> String {
    format!("{}.{}.{}", version.0, version.1, version.2)
}
