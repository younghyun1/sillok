# Agent skill

Agents learn the Sillok workflow from a skill file. That file used to be maintained by hand outside this repository, so it drifted from the binary it described. It now ships inside the binary.

## One source

`src/guide.md` is the workflow. `sillok guide` prints it, and `skill::content::render` wraps it in `SKILL.md` frontmatter: `name`, `description` (the trigger agents match on), and a `metadata` map holding `sillok-version` and `sillok-digest`. The digest is a UUIDv5 over the file without its own line. It is edit detection, not security: a file whose digest still matches was written by Sillok and not changed since.

## Targets

`skill::roots` maps each tool to exactly one file, because a tool that finds the skill in two roots loads it twice.

| Tool | Root |
| --- | --- |
| Claude Code | `$CLAUDE_CONFIG_DIR` or `~/.claude`, then `skills/` |
| Codex | `$CODEX_HOME` or `~/.codex`, then `skills/`; else `~/.agents/skills` |

Codex reads both of its roots. The root that already holds the skill wins; with none installed, an existing root is preferred over creating one. When both hold a copy, the tool's own root is the target and the other is reported in `warnings`; Sillok does not delete files it was not asked to remove. `--dir <root>` targets any other skill root, such as a project's `.claude/skills`. Without `--tool` or `--dir`, install covers the tools whose home directory exists.

## Install and refresh

`sillok skill install` is explicit, so it converges each target on this binary's skill whatever is there: `installed`, `updated`, or `unchanged` (no write). Writes go through a sibling temporary file and a rename.

`sillok status` is the command every session starts with, so it is where upgrades propagate. For each user-level target it reads the file and acts on its state:

| State | Meaning | On `status` |
| --- | --- | --- |
| `current` | Identical to this binary's skill | Nothing |
| `outdated` | Stamped, digest intact, version not newer | Replaced; one notice in `warnings` |
| `newer` | Stamped, digest intact, newer version | Nothing, so two binaries on one machine do not flap |
| `modified` | Stamped, digest broken | Left alone; a notice while it predates the binary |
| `unmanaged` | No stamp | Nothing; only `skill install` replaces it |
| `missing` | No file | Nothing |

The refresh costs one small file read per tool, never fails the command, and is disabled by `SILLOK_SKILL_AUTOUPDATE=0`. Project-level copies installed with `--dir` are not tracked; re-run the install after an upgrade.
