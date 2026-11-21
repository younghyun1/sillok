//! Where each agent tool reads user-level skills.
//!
//! One tool gets exactly one copy. Codex reads two roots, its own home and
//! the shared `~/.agents/skills`; installing into both would make it
//! discover the skill twice, so the root that already holds a copy wins and
//! a second copy is reported instead of updated.

use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::skill::content::{SKILL_FILE, SKILL_NAME};

/// Overrides the Claude Code home, as Claude Code itself does.
pub const CLAUDE_HOME_ENV: &str = "CLAUDE_CONFIG_DIR";

/// Overrides the Codex home, as Codex itself does.
pub const CODEX_HOME_ENV: &str = "CODEX_HOME";

/// An agent tool with a known user-level skill root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Codex,
}

impl Tool {
    /// Every supported tool.
    pub const ALL: [Self; 2] = [Self::Claude, Self::Codex];

    /// Stable label used in output.
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

/// One place the skill is, or would be, installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Tool label, or `dir` for an explicit skill root.
    pub tool: &'static str,
    /// The tool, when the target is one's user-level root.
    pub kind: Option<Tool>,
    /// Path of the `SKILL.md` to write.
    pub path: PathBuf,
    /// A second copy the same tool would also discover.
    pub duplicate: Option<PathBuf>,
}

/// Tool home directories on this machine.
#[derive(Debug, Clone)]
pub struct Homes {
    claude: PathBuf,
    codex: PathBuf,
    /// `~/.agents/skills`, the cross-tool root Codex also reads.
    shared: PathBuf,
}

impl Homes {
    /// Resolves the homes from the environment; `None` without a home directory.
    pub fn from_env() -> Option<Self> {
        let home = match BaseDirs::new() {
            Some(dirs) => dirs.home_dir().to_path_buf(),
            None => return None,
        };
        Some(Self {
            claude: env_dir(CLAUDE_HOME_ENV, || home.join(".claude")),
            codex: env_dir(CODEX_HOME_ENV, || home.join(".codex")),
            shared: home.join(".agents").join("skills"),
        })
    }

    /// Builds homes from explicit directories.
    pub fn new(claude: PathBuf, codex: PathBuf, shared: PathBuf) -> Self {
        Self {
            claude,
            codex,
            shared,
        }
    }

    /// Whether the tool appears to be set up on this machine.
    pub fn has(&self, tool: Tool) -> bool {
        match tool {
            Tool::Claude => self.claude.is_dir(),
            Tool::Codex => self.codex.is_dir(),
        }
    }

    /// The file a tool reads its hooks from.
    pub fn hook_config(&self, tool: Tool) -> PathBuf {
        match tool {
            Tool::Claude => self.claude.join("settings.json"),
            Tool::Codex => self.codex.join("hooks.json"),
        }
    }

    /// The single install target of a tool.
    pub fn target(&self, tool: Tool) -> Target {
        match tool {
            Tool::Claude => Target {
                tool: tool.label(),
                kind: Some(tool),
                path: skill_file(&self.claude.join("skills")),
                duplicate: None,
            },
            Tool::Codex => {
                let own = skill_file(&self.codex.join("skills"));
                let shared = skill_file(&self.shared);
                let (path, duplicate) = match (own.is_file(), shared.is_file()) {
                    (true, true) => (own, Some(shared)),
                    (true, false) => (own, None),
                    (false, true) => (shared, None),
                    // Nothing installed yet: prefer a root that already exists.
                    (false, false) => {
                        match !self.codex.join("skills").is_dir() && self.shared.is_dir() {
                            true => (shared, None),
                            false => (own, None),
                        }
                    }
                };
                Target {
                    tool: tool.label(),
                    kind: Some(tool),
                    path,
                    duplicate,
                }
            }
        }
    }

    /// Targets of every tool set up on this machine.
    pub fn detected(&self) -> Vec<Target> {
        Tool::ALL
            .into_iter()
            .filter(|tool| self.has(*tool))
            .map(|tool| self.target(tool))
            .collect()
    }
}

/// Target for an explicit skill root such as a project's `.claude/skills`.
pub fn dir_target(root: &Path) -> Target {
    Target {
        tool: "dir",
        kind: None,
        path: skill_file(root),
        duplicate: None,
    }
}

fn skill_file(root: &Path) -> PathBuf {
    root.join(SKILL_NAME).join(SKILL_FILE)
}

fn env_dir(name: &str, default: impl FnOnce() -> PathBuf) -> PathBuf {
    match std::env::var_os(name) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        Some(_) | None => default(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Homes, Tool, dir_target};
    use crate::error::SillokError;

    fn homes(root: &Path) -> Homes {
        Homes::new(
            root.join("claude"),
            root.join("codex"),
            root.join("agents").join("skills"),
        )
    }

    fn touch(path: &Path) -> Result<(), SillokError> {
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            return Err(error.into());
        }
        match std::fs::write(path, "x") {
            Ok(()) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    #[test]
    fn only_tools_with_a_home_are_detected() -> Result<(), SillokError> {
        let dir = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let homes = homes(dir.path());
        assert!(homes.detected().is_empty());
        if let Err(error) = std::fs::create_dir_all(dir.path().join("claude")) {
            return Err(error.into());
        }
        let found = homes.detected();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tool, "claude");
        assert!(found[0].path.ends_with("claude/skills/sillok/SKILL.md"));
        Ok(())
    }

    #[test]
    fn codex_gets_one_copy_and_reports_a_second() -> Result<(), SillokError> {
        let dir = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let homes = homes(dir.path());
        let own = dir.path().join("codex/skills/sillok/SKILL.md");
        let shared = dir.path().join("agents/skills/sillok/SKILL.md");
        // Nothing anywhere: the tool's own root.
        assert_eq!(homes.target(Tool::Codex).path, own);
        // Only the shared root exists: use it instead of creating a second root.
        if let Err(error) = std::fs::create_dir_all(dir.path().join("agents/skills")) {
            return Err(error.into());
        }
        assert_eq!(homes.target(Tool::Codex).path, shared);
        // An existing copy pins its root.
        if let Err(error) = touch(&shared) {
            return Err(error);
        }
        assert_eq!(homes.target(Tool::Codex).path, shared);
        assert_eq!(homes.target(Tool::Codex).duplicate, None);
        // Two copies: keep the tool's own and name the other.
        if let Err(error) = touch(&own) {
            return Err(error);
        }
        let target = homes.target(Tool::Codex);
        assert_eq!(target.path, own);
        assert_eq!(target.duplicate, Some(shared));
        Ok(())
    }

    #[test]
    fn explicit_roots_get_the_skill_directory() {
        let target = dir_target(Path::new(".claude/skills"));
        assert_eq!(target.tool, "dir");
        assert!(target.path.ends_with(".claude/skills/sillok/SKILL.md"));
    }
}
