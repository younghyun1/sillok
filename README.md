# Sillok

Sillok is a structured chronicle of agent work: objectives, the tasks done under them, and what happened each day. Coding agents call it while they work so progress survives context loss, sessions, and machines. The name comes from the Joseon *sillok*, court chronicles that recorded what happened, when, and why.

## Install

```bash
cargo install sillok      # or, from a checkout: cargo install --path .
sillok skill install --hooks   # install the agent skill and hooks for Claude Code and Codex
sillok guide              # the same workflow, printed
```

A checkout builds for the host CPU on nightly (`.cargo/config.toml`, `rust-toolchain.toml`); the crates.io build uses stable defaults. The first command after upgrading from 0.9 or 0.10 migrates the existing store in place; see [Upgrading from 0.x](#upgrading-from-0x).

Windows builds need the Visual Studio C++ build tools and Windows SDK for bundled SQLite and zstd; no Unix shell is required.

## For agents

The agent workflow ships inside the binary, so it always matches the installed version. `sillok skill install` writes it as a skill (`sillok/SKILL.md`) where each tool loads user-level skills:

| Tool | Location |
| --- | --- |
| Claude Code | `~/.claude/skills`, or under `CLAUDE_CONFIG_DIR` |
| Codex | `~/.codex/skills` (or under `CODEX_HOME`), else `~/.agents/skills` when that root already exists or holds the skill |
| Anything else | `sillok skill install --dir <skill root>`, for example a project's `.claude/skills` |

Install is idempotent: it replaces an existing copy, writes nothing when the copy is current, and keeps one copy per tool, naming a second one it finds instead of updating both. After an upgrade, `sillok status` refreshes user-level copies that Sillok wrote and nobody edited; a hand-edited copy is left alone and reported. `SILLOK_SKILL_AUTOUPDATE=0` turns the refresh off, and `sillok skill status` shows what is installed.

`--hooks` also registers `sillok hook` with each tool, in `~/.claude/settings.json` and `~/.codex/hooks.json`:

- **Session start**: the repository's status arrives as session context, so resuming costs no tool call, and on Claude Code `SILLOK_SESSION` is exported for the session so events carry its label.
- **End of turn**: a turn that changed files or systems without writing to Sillok is sent back once with a reminder; a turn that recorded, or changed nothing, ends normally.

Registration leaves every other setting alone, keeps one Sillok handler per event however often it runs, and replaces entries for the standalone `sillok-reminder` binary this supersedes. A hook failure is logged to stderr and never blocks a session. Codex runs new hooks only after you trust them under `/hooks`.

For tools without skills, add this to a repository's `AGENTS.md` or `CLAUDE.md`:

````markdown
## Sillok
Record objectives, completed work, and corrections with Sillok while you work; do not rely on chat history. Run `sillok status` at session start to resume, and `sillok guide` for the full workflow.

```bash
obj=$(sillok objective add "Ship the storage refactor" --tags rust)
sillok note "Split reducer from indexing" --parent "$obj" --tags rust
sillok amend <id> --status completed --note "Root cause was clock skew"
sillok objective complete "$obj" --note "Scoped work is done"
sillok day
```

Never run `sillok reset --yes` unless the user explicitly asks to erase the chronicle.
````

The CLI is shaped for that caller:

- **Writes print the affected id** and nothing else, so `id=$(sillok note ...)` works. Ids can be abbreviated to a unique prefix of eight or more hex digits, and `note --stdin` records several tasks from JSON lines in one transaction.
- **Reads print compact JSON**: empty fields are omitted and the work context appears only with `--full`. `--json` adds the `{ok, command, generated_at, data, warnings}` envelope and `--human` prints text.
- **Errors are JSON on stderr**, `{"error":"code","message":"..."}`, with exit code 1, or 2 for usage errors. Codes are stable across 1.x.
- **Concurrency is safe**: many agents can read and write one store at once; SQLite WAL mode lets readers proceed during a write and makes writers wait instead of failing.
- **Resuming is one command**: `sillok status` lists open objectives and recent work for the current repository, including work recorded in its other worktrees and in other clones of the same remote, whichever URL each clone uses.

## Model

Records are objectives or tasks. Any record can sit under another (objectives only under objectives), so an objective can collect work across many days. Days are not stored: `sillok day --date D` shows every record that had an event on D in the chosen timezone, with an `activity` list (`recorded`, `amended`, `completed`, `moved`, `retracted`, `restored`) and its ancestors for context. A task started Monday and finished Wednesday appears on both days.

Every change is an immutable event; current records are a projection that `sillok doctor` checks against a full replay and `doctor --repair` rebuilds. Retraction hides a record but keeps its history, and `restore` brings it back. Status `retracted` can only be set by `retract`, which requires a reason.

## Commands

| Command | Purpose |
| --- | --- |
| `note <text> [--parent] [--status] [--tags] [--purpose]`, `note --stdin` | Record a task (default `completed`), or one per JSON line |
| `objective add <text> [--parent] [--tags] [--status]` | Start an objective (default `active`) |
| `objective complete <id> [--note]` | Complete an objective |
| `objective list [--all] [--limit]` | Open objectives, newest 20 by default |
| `amend <id> [--text] [--status] [--purpose\|--clear-purpose] [--tags\|--clear-tags] [--note]` | Change fields |
| `move <id> (--parent <id>\|--top)` | Re-parent; cycles are rejected |
| `retract <id> --reason` / `restore <id>` | Hide or unhide |
| `show <id>` | Record, children, and full event history |
| `day [--date]` | A day as a tree with activity |
| `tree <id>` | A record and its visible descendants |
| `query [--since\|--from/--to\|--date] [--tag] [--status] [--kind] [--open] [--text] [--context] [--limit]` | Search |
| `status [--all] [--limit]` | Open objectives and recent work in this repository |
| `export [--events] [--from] [--to]` | JSON lines: records, or raw events (the portable archive) |
| `import <path> [--dry-run]` | Merge a 0.x store or archive, an events export, or a sync directory |
| `doctor [--repair] [--prune]` | Integrity check and replay comparison; `--prune` deletes backup files beside the store and leftover sync directories |
| `sync [--dry-run]`, `sync remote set <url> [--branch] [--dir]`, `sync remote show`, `sync auto <minutes>` | Git sync |
| `reset --yes` | Back up, then empty the store |
| `guide` | Agent usage guide |
| `skill install [--tool claude\|codex] [--dir <root>] [--hooks]`, `skill status`, `skill print` | Install, inspect, or print the agent skill; `--hooks` registers the hooks |
| `hook` | Handle one Claude Code or Codex hook event from stdin |

Global options: `--store`, `--tz`, `--at` (backfill: RFC 3339, or `YYYY-MM-DDTHH:MM[:SS]` in `--tz`), `--full`, `--json`, `--human`.

Environment: `SILLOK_STORE` (default `$XDG_DATA_HOME/sillok/sillok.db`), `SILLOK_TZ` (default: system zone), `SILLOK_ACTOR` (default `agent`), `SILLOK_SESSION` (recorded on each event), `SILLOK_OUTPUT` (`compact`, `json`, or `human`), `SILLOK_SKILL_AUTOUPDATE` (`0` stops `status` from refreshing installed skills), `SILLOK_AUTOSYNC` (`0` stops `status` from starting a background sync), `SILLOK_LOG` (tracing filter; JSON logs go to stderr). An empty variable counts as unset.

The 0.10 spellings `tree --root`, `export json`, `migrate`, `truncate`, and `sync run` still work as hidden aliases.

## Sync

```bash
sillok sync remote set git@github.com:you/sillok-archive.git
sillok sync
```

Once a remote is set, `sillok status` also starts a sync in the background when the last one is more than an hour old, and returns without waiting; a failure shows up in the next `status` under `warnings`. `sillok sync auto <minutes>` changes the interval, and `0` (or `SILLOK_AUTOSYNC=0`) turns background sync off.

The remote holds `sillok/manifest.json` and one plain-text JSON-lines file per month (`sillok/events/2026-09.jsonl`), sorted by recorded time. New events append to the current month, so each sync commit is a small readable diff and Git's own compression does the rest. Sync unions events by id: pulls what the local store lacks, pushes what the remote lacks, and rewrites only the months that changed. Events of a type this version does not know are kept and pushed back byte for byte. If one event id carries different bytes on two machines, both keep the lexicographically smaller bytes and report a warning; sync never stops for a human to resolve a conflict. Git runs non-interactively (`GIT_TERMINAL_PROMPT=0`, SSH `BatchMode`), so missing credentials fail fast instead of hanging an agent.

## Upgrading from 0.x

The first command run against a 0.9 or 0.10 store migrates it: events are converted to the 1.0 format, the old database is kept as `sillok.db.v2-<ms>.bak.db`, and the command's output carries a one-line notice. Concurrent first runs wait for one migration. On the first sync, the 0.10 artifact (`sillok.slk.zst`) is imported, the remote switches to the monthly layout, and the old file is removed. Upgrade every machine; a 0.10 binary cannot read a 1.0 store or remote.

The conversion is deterministic, so machines that migrate independently produce identical events and sync cleanly. Accepted differences from 0.10:

- Day records disappear; records that were directly under a day become top-level.
- A task recorded under a parent from an earlier day now shows on the day it was recorded, not the parent's day.
- An objective's completion note moves from `purpose` to `note`.
- Git remote URLs lose any embedded `user:token@` credentials.

`sillok import <file>` also accepts a v1 `sillok.slk.zst` archive or a 0.10 sync artifact directly.

## Compatibility

Semantic versioning covers the command line (commands, flags, output fields, error codes, exit codes), the event format, and the sync layout. The Rust library target exists for tests and tools and is not covered. Event format evolution rules are in [docs/architecture/be/event-format.md](docs/architecture/be/event-format.md).

## Development

```bash
cargo fmt
cargo clippy --all-targets
cargo test
cargo run --example store_probe   # 50k-record latency probe
```

Conventions: no `unwrap`, `expect`, or `?`; errors are handled with explicit `match`. Files stay under 300 lines and every `mod.rs` only declares modules. Backend design notes live in [docs/architecture/be](docs/architecture/be), plans in [docs/planning](docs/planning).
