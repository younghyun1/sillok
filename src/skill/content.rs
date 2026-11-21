//! The agent skill embedded in the binary.
//!
//! `guide.md` is the single source of the agent workflow: `sillok guide`
//! prints it, and `render` wraps it in `SKILL.md` frontmatter stamped with
//! this binary's version and a digest of the file. The stamp lets a later
//! binary tell a copy it wrote and nobody edited, which is safe to replace,
//! from one a person changed.

use uuid::Uuid;

/// The agent workflow, printed by `sillok guide` and embedded in the skill.
pub const GUIDE: &str = include_str!("../guide.md");

/// Skill name; also the directory under a skill root.
pub const SKILL_NAME: &str = "sillok";

/// File name of a skill inside its directory.
pub const SKILL_FILE: &str = "SKILL.md";

/// Version of this binary, stamped into the skill.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Trigger text agents use to decide when to load the skill. It is written
/// as a plain YAML scalar, so it must not contain `: ` or ` #`.
const DESCRIPTION: &str = "Sillok work chronicle. Load at the start of every session, before any substantive work; covers resuming with `sillok status`, recording objectives, tasks, and corrections, reading days and history, querying, and Git sync. Requires `sillok` on PATH.";

const VERSION_KEY: &str = "  sillok-version: ";
const DIGEST_KEY: &str = "  sillok-digest: ";

/// Namespace of the edit-detection digest (a UUIDv5 over the file text).
const DIGEST_NAMESPACE: Uuid = Uuid::from_u128(0x7a0e_52c1_93b4_4d0f_8e6a_1f3d_b25c_9a71);

/// `major.minor.patch`.
pub type Version = (u64, u64, u64);

/// What the frontmatter of an installed skill says about its origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// Version of the binary that wrote the file.
    pub version: Version,
    /// Whether the text still matches its digest, meaning nobody edited it.
    pub pristine: bool,
}

/// Renders the complete `SKILL.md` for this binary.
pub fn render() -> String {
    render_as(VERSION)
}

/// Renders the skill as the binary of `version` would stamp it.
pub fn render_as(version: &str) -> String {
    let digest = digest(&compose(version, None));
    compose(version, Some(&digest))
}

/// Reads the stamp of an installed skill; `None` when Sillok did not write it.
pub fn inspect(text: &str) -> Option<Stamp> {
    let mut version = None;
    let mut recorded = None;
    let mut unstamped = String::with_capacity(text.len());
    let mut lines = text.split_inclusive('\n');
    match lines.next() {
        Some(first) if first.trim_end() == "---" => unstamped.push_str(first),
        Some(_) | None => return None,
    }
    let mut in_frontmatter = true;
    for line in lines {
        if in_frontmatter {
            if line.trim_end() == "---" {
                in_frontmatter = false;
            } else if let Some(value) = line.strip_prefix(DIGEST_KEY) {
                // The digest covers every other byte, so its own line is left out.
                recorded = Some(unquote(value));
                continue;
            } else if let Some(value) = line.strip_prefix(VERSION_KEY) {
                version = parse_version(unquote(value));
            }
        }
        unstamped.push_str(line);
    }
    match (version, recorded) {
        (Some(version), Some(recorded)) => Some(Stamp {
            version,
            pristine: digest(&unstamped) == recorded,
        }),
        (Some(_), None) | (None, Some(_)) | (None, None) => None,
    }
}

/// Parses `major.minor.patch`, ignoring any pre-release or build suffix.
pub fn parse_version(raw: &str) -> Option<Version> {
    let core = match raw.split(['-', '+']).next() {
        Some(value) => value,
        None => return None,
    };
    let mut parts = core.split('.').map(str::parse::<u64>);
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch)), None) => Some((major, minor, patch)),
        _ => None,
    }
}

fn compose(version: &str, digest: Option<&str>) -> String {
    let mut out = String::with_capacity(GUIDE.len() + 512);
    out.push_str("---\n");
    out.push_str(&format!("name: {SKILL_NAME}\n"));
    out.push_str(&format!("description: {DESCRIPTION}\n"));
    out.push_str("metadata:\n");
    out.push_str(&format!("{VERSION_KEY}\"{version}\"\n"));
    if let Some(value) = digest {
        out.push_str(&format!("{DIGEST_KEY}\"{value}\"\n"));
    }
    out.push_str("---\n\n");
    out.push_str(GUIDE.trim_end());
    out.push('\n');
    out
}

fn digest(text: &str) -> String {
    Uuid::new_v5(&DIGEST_NAMESPACE, text.as_bytes())
        .simple()
        .to_string()
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"')
}

#[cfg(test)]
mod tests {
    use super::{DESCRIPTION, GUIDE, Stamp, VERSION, inspect, parse_version, render};

    #[test]
    fn rendered_skill_is_stamped_and_pristine() {
        let text = render();
        assert!(text.starts_with("---\nname: sillok\ndescription: "));
        assert!(text.ends_with(&format!("{}\n", GUIDE.trim_end())));
        let version = match parse_version(VERSION) {
            Some(value) => value,
            None => panic!("package version `{VERSION}` is not major.minor.patch"),
        };
        assert_eq!(
            inspect(&text),
            Some(Stamp {
                version,
                pristine: true,
            })
        );
        assert_eq!(render(), text);
    }

    #[test]
    fn edits_and_foreign_files_are_detected() {
        let edited = render().replace("Record as you go", "Record whenever");
        assert_eq!(inspect(&edited).map(|stamp| stamp.pristine), Some(false));
        assert_eq!(inspect("---\nname: sillok\n---\n\nHand written.\n"), None);
        assert_eq!(inspect("# No frontmatter\n"), None);
        assert_eq!(inspect(""), None);
    }

    #[test]
    fn description_is_a_plain_yaml_scalar() {
        assert!(!DESCRIPTION.contains(": "));
        assert!(!DESCRIPTION.contains(" #"));
        assert!(!DESCRIPTION.contains('\n'));
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(parse_version("1.10.2"), Some((1, 10, 2)));
        assert_eq!(parse_version("2.0.0-rc.1"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.1"), None);
        assert_eq!(parse_version("1.1.0.4"), None);
        assert!(parse_version("1.10.0") > parse_version("1.9.9"));
    }
}
