//! Arguments of the `skill` commands.

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use crate::skill::roots::Tool;

/// Agent tools with a known user-level skill root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ToolArg {
    /// Claude Code: `~/.claude/skills`, or under `CLAUDE_CONFIG_DIR`.
    Claude,
    /// Codex: `~/.codex/skills` (or under `CODEX_HOME`), else `~/.agents/skills`.
    Codex,
}

impl ToolArg {
    /// Domain tool.
    pub fn tool(self) -> Tool {
        match self {
            Self::Claude => Tool::Claude,
            Self::Codex => Tool::Codex,
        }
    }
}

/// `skill`.
#[derive(Debug, Args)]
pub struct SkillArgs {
    #[command(subcommand)]
    pub command: SkillCommand,
}

/// `skill` subcommands.
#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// Write the skill where agent tools load it, replacing any existing copy.
    Install(SkillInstallArgs),
    /// Where the skill is installed and whether each copy is current.
    Status,
    /// Print the SKILL.md this binary installs.
    Print,
}

/// `skill install`.
#[derive(Debug, Args)]
pub struct SkillInstallArgs {
    /// Tool to install for; repeatable. Default: every tool set up on this machine.
    #[arg(long = "tool", value_enum, value_name = "TOOL")]
    pub tools: Vec<ToolArg>,
    /// Skill root to install into, such as a project's `.claude/skills`; repeatable.
    #[arg(long = "dir", value_name = "SKILL_ROOT")]
    pub dirs: Vec<PathBuf>,
}
