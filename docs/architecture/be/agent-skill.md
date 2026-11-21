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

## Hooks

`sillok hook` is one entry point for the events both tools send as a JSON object on stdin, and `skill install --hooks` registers it. `skill::hooks` edits the tool's hook file (`settings.json` for Claude Code, `hooks.json` for Codex) as JSON: for `SessionStart`, `PostToolUse`, and `Stop` it removes handlers whose command names `sillok hook` or the earlier standalone `sillok-reminder`, drops groups that removal emptied, and appends one handler. The file is rewritten only when the result differs, so a second run reports `unchanged`, and no other key or handler is touched. A file that is not valid JSON is refused, not overwritten. The handler command is guarded with `command -v sillok` and ends in `|| exit 0`, so a machine that shares the configuration without the binary, or with one older than 1.2, runs a no-op instead of failing every tool call.

| Event | Behaviour |
| --- | --- |
| `SessionStart` | Runs `status` for the payload's working directory with five recent records and returns it as `hookSpecificOutput.additionalContext`, trimmed oldest-first to 8,000 characters (Claude Code caps context at 10,000). When `CLAUDE_ENV_FILE` is set, appends `export SILLOK_SESSION=<session id>` once. Subagent sessions are ignored. |
| `PostToolUse` | Classifies the tool call (`hook::reminder`) and appends to a per-session ledger whether it changed files or systems and whether it wrote to Sillok. |
| `Stop` | Reads and clears the ledger. A turn with a change and no Sillok write gets `{"decision":"block","reason":...}`, which both tools treat as a continuation prompt. A Stop that is already a continuation always ends the turn, so the reminder fires once. |

A change is a file edit or patch outside the temporary directory; a shell redirection or in-place writer (`sed -i`, `sd`, `tee`, `cp`, `mv`, `rm`) outside it, following `cd` and variables the script sets; a mutating subcommand of `git`, `gh`, `gcloud`, `gsutil`, `bq`, `kubectl`, `helm`, `tofu`, `gam`, `op`, `aws`, `az`, `psql`, or `curl`; an MCP tool named for a write; a browser click, keystroke, or form fill; or an Artifact publish. Reads are not listed, so a missed change costs one silent turn instead of a spurious reminder. A Sillok write is `note`, `amend`, `move`, `retract`, `restore`, `import`, or `objective add|complete`, in any shell form including `$(...)` and `bash -c`.

The hook prints only what the host parses, regardless of `SILLOK_OUTPUT`, reads at most 8 MiB with a two-second stdin deadline, and exits successfully on every failure after logging it to stderr. The ledger lives under the temporary directory (`SILLOK_REMINDER_DIR` overrides it), is bounded per session, and holds short labels such as `git push`, never argument values or tool output.
