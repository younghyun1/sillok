//! Resolve written paths and discount scratch space, so temporary files are not work.

use std::path::{Component, Path, PathBuf};

/// Characters kept from a file name quoted back in a reminder.
const LABEL_CHARS: usize = 60;

/// Label for a path the turn wrote outside scratch space; `None` for scratch,
/// device, and empty targets.
pub fn written(raw: &str, cwd: Option<&Path>) -> Option<String> {
    if raw.is_empty() || raw.starts_with("/dev/") {
        return None;
    }
    // Variables the script did not assign itself usually hold a scratch location
    // from an earlier call; they cannot be resolved statically, so they do not count.
    if raw.contains('$') {
        return None;
    }
    match resolve(raw, cwd) {
        Some(path) if scratch(&path) => None,
        Some(_) | None => Some(label(raw)),
    }
}

/// Absolute, lexically normalized form of `raw`, or `None` when a relative path has
/// no working directory or `~` has no home.
fn resolve(raw: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let joined = if raw == "~" || raw.starts_with("~/") {
        match std::env::var_os("HOME") {
            Some(home) => {
                PathBuf::from(home).join(raw.trim_start_matches('~').trim_start_matches('/'))
            }
            None => return None,
        }
    } else {
        let path = Path::new(raw);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            match cwd {
                Some(cwd) => cwd.join(path),
                None => return None,
            }
        }
    };
    Some(normalize(&joined))
}

/// Fold `.` and `..` without touching the filesystem, so `/tmp/../etc` is not scratch.
fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => (),
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                result.push(component.as_os_str());
            }
        }
    }
    result
}

/// The per-user temporary directory, plus the Unix temp roots that macOS reaches
/// through both `/tmp` and its `/private` target.
fn scratch(path: &Path) -> bool {
    if path.starts_with(normalize(&std::env::temp_dir())) {
        return true;
    }
    cfg!(unix)
        && [
            "/tmp",
            "/private/tmp",
            "/var/tmp",
            "/private/var/tmp",
            "/var/folders",
            "/private/var/folders",
        ]
        .iter()
        .any(|root| path.starts_with(root))
}

/// Last path component, bounded on a character boundary.
fn label(raw: &str) -> String {
    let name = match raw.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next() {
        Some(name) if !name.is_empty() => name,
        Some(_) | None => raw,
    };
    name.chars().take(LABEL_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_and_device_paths_do_not_count() {
        let cwd = Path::new("/work/repo");
        assert_eq!(written("/dev/null", Some(cwd)), None);
        assert_eq!(written("$TMPDIR/out.txt", Some(cwd)), None);
        assert_eq!(written("$OUT/x", Some(cwd)), None);
        let temp = std::env::temp_dir().join("scratch.md");
        assert_eq!(written(&temp.to_string_lossy(), Some(cwd)), None);
        if cfg!(unix) {
            assert_eq!(written("/private/tmp/claude/x.json", Some(cwd)), None);
            assert_eq!(written("x.json", Some(Path::new("/tmp/work"))), None);
        }
    }

    #[test]
    fn project_paths_count_with_a_short_label() {
        let cwd = Path::new("/work/repo");
        assert_eq!(
            written("docs/plan.md", Some(cwd)),
            Some("plan.md".to_owned())
        );
        assert_eq!(
            written("/work/repo/src/", Some(cwd)),
            Some("src".to_owned())
        );
        assert_eq!(
            written("relative.txt", None),
            Some("relative.txt".to_owned())
        );
        if cfg!(unix) {
            assert_eq!(
                written("/tmp/../etc/hosts", Some(cwd)),
                Some("hosts".to_owned())
            );
        }
        let long = format!("/work/{}", "é".repeat(LABEL_CHARS + 5));
        assert_eq!(
            written(&long, Some(cwd)).map(|l| l.chars().count()),
            Some(LABEL_CHARS)
        );
    }
}
