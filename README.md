# Sillok

Sillok is a structured chronicle of agent work: objectives, the tasks done under them, and what happened each day. Coding agents call it while they work so progress survives context loss, sessions, and machines. The name comes from the Joseon *sillok*, court chronicles that recorded what happened, when, and why.

## Install

```bash
cargo install --path .
sillok guide        # the agent workflow in one page
```

The binary is built for the host CPU (`.cargo/config.toml`). The first command after upgrading from 0.9 or 0.10 migrates the existing store in place; see [Upgrading from 0.x](#upgrading-from-0x).

## For agents

Add this to a repository's `AGENTS.md` or `CLAUDE.md`:

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

- **Writes print the affected id** and nothing else, so `id=$(sillok note ...)` works.
- **Reads print compact JSON**: empty fields are omitted and the work context appears only with `--full`. `--json` adds the `{ok, command, generated_at, data, warnings}` envelope and `--human` prints text.
- **Errors are JSON on stderr**, `{"error":"code","message":"..."}`, with exit code 1, or 2 for usage errors. Codes are stable across 1.x.
- **Concurrency is safe**: many agents can read and write one store at once; SQLite WAL mode lets readers proceed during a write and makes writers wait instead of failing.
- **Resuming is one command**: `sillok status` lists open objectives and recent work for the current repository.

## Model

Records are objectives or tasks. Any record can sit under another (objectives only under objectives), so an objective can collect work across many days. Days are not stored: `sillok day --date D` shows every record that had an event on D in the chosen timezone, with an `activity` list (`recorded`, `amended`, `completed`, `moved`, `retracted`, `restored`) and its ancestors for context. A task started Monday and finished Wednesday appears on both days.

Every change is an immutable event; current records are a projection that `sillok doctor` checks against a full replay and `doctor --repair` rebuilds. Retraction hides a record but keeps its history, and `restore` brings it back. Status `retracted` can only be set by `retract`, which requires a reason.

## Commands

| Command | Purpose |
| --- | --- |
| `note <text> [--parent] [--status] [--tags] [--purpose]` | Record a task (default `completed`) |
| `objective add <text> [--parent] [--tags] [--status]` | Start an objective (default `active`) |
| `objective complete <id> [--note]` | Complete an objective |
| `objective list [--all] [--limit]` | Open objectives |
| `amend <id> [--text] [--status] [--purpose\|--clear-purpose] [--tags\|--clear-tags] [--note]` | Change fields |
| `move <id> (--parent <id>\|--top)` | Re-parent; cycles are rejected |
| `retract <id> --reason` / `restore <id>` | Hide or unhide |
| `show <id>` | Record, children, and full event history |
| `day [--date]` | A day as a tree with activity |
| `tree <id>` | A record and its visible descendants |
| `query [--since\|--from/--to\|--date] [--tag] [--status] [--kind] [--open] [--text] [--context] [--limit]` | Search |
| `status [--all] [--limit]` | Open objectives and recent work here |
| `export [--events] [--from] [--to]` | JSON lines: records, or raw events (the portable archive) |
| `import <path> [--dry-run]` | Merge a 0.x store or archive, an events export, or a sync directory |
| `doctor [--repair]` | Integrity check and replay comparison |
| `sync [--dry-run]`, `sync remote set <url> [--branch] [--dir]`, `sync remote show` | Git sync |
| `reset --yes` | Back up, then empty the store |
| `guide` | Agent usage guide |

Global options: `--store`, `--tz`, `--at` (backfill: RFC 3339, or `YYYY-MM-DDTHH:MM[:SS]` in `--tz`), `--full`, `--json`, `--human`.

Environment: `SILLOK_STORE` (default `$XDG_DATA_HOME/sillok/sillok.db`), `SILLOK_TZ` (default: system zone), `SILLOK_ACTOR` (default `agent`), `SILLOK_SESSION` (recorded on each event), `SILLOK_OUTPUT` (`compact`, `json`, or `human`), `SILLOK_LOG` (tracing filter; JSON logs go to stderr).

The 0.10 spellings `tree --root`, `export json`, `migrate`, `truncate`, and `sync run` still work as hidden aliases.

## Sync

```bash
sillok sync remote set git@github.com:you/sillok-archive.git
sillok sync
```

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
