# Sillok

Sillok is a Rust CLI for meticulous agentic work chronicles. It records daily
objectives, completed tasks, corrections, and retractions into a structured
archive instead of a fragile append-only text log.

The name comes from Korean `sillok`, the court chronicles of Joseon. The design
goal is similar: keep precise records of what happened, when, under what
objective, and in what working context.

## Status

Sillok is an early local-first CLI. The command interface is intended to be
stable enough for agent harnesses. The live store is a private Turso/SQLite
database and may gain new datashape versions over time.

## Install

Build the binary in dev mode:

```bash
cargo build
```

Install it onto your PATH:

```bash
cargo install --path .
```

Then use:

```bash
sillok --help
```

## Storage

By default, Sillok stores one user-global Turso/SQLite database at:

```text
$XDG_DATA_HOME/sillok/sillok.db
```

If `XDG_DATA_HOME` is unset, it falls back to:

```text
~/.local/share/sillok/sillok.db
```

Override the store with either:

```bash
sillok --store /path/to/sillok.db day
SILLOK_STORE=/path/to/sillok.db sillok day
```

The v2 store keeps append-only events plus indexed current projections in the
database. Normal mutations insert a new event and update affected projection
rows instead of loading and rewriting the whole chronicle.

Legacy v1 stores used a compressed private archive at `sillok.slk.zst`. Use
`sillok migrate --store /path/to/sillok.slk.zst --target /path/to/sillok.db --yes`
to create a v2 database. Migration always validates the legacy archive and
creates a timestamped backup before activating the target.

Git sync is archive-based, not database-based. The configured remote stores one
`bitcode` + zstd artifact, while every device keeps its own local SQLite/Turso
projection database. Sync config is stored beside the selected store at
`<store>.sync.json`.

## Output

Quiet output is the default and is intended for agents. Successful write/action
commands usually print nothing:

```bash
sillok note "Implemented archive-backed task logging"
```

Read commands still return compact JSON data without the response envelope. Use
`--json` when an agent needs IDs or the full verbose response envelope:

```json
{
  "ok": true,
  "command": "note",
  "generated_at": "2026-05-13T10:00:00+00:00",
  "ids": {},
  "data": {},
  "warnings": []
}
```

Failures are compact JSON by default:

```json
{"error":"sync_remote_missing","message":"sync remote is not configured"}
```

Use `--json` for verbose failure envelopes:

```json
{
  "ok": false,
  "command": "sync",
  "generated_at": "2026-05-13T10:00:00+00:00",
  "error": {
    "code": "sync_remote_missing",
    "message": "sync remote is not configured"
  }
}
```

JSON records include stable RFC3339 `created_at` and `updated_at` fields.
Verbose success responses include `ids`, `data`, and `warnings`. Use `--human`
for interactive summaries with local-device timestamps rendered as readable
wall-clock time:

```bash
sillok --human day
```

## Functionality

Sillok supports:

- Local-first event logging into a SQLite/Turso projection store.
- Daily task notes with status, tags, purpose text, and parent links.
- Day objectives that can be created and completed with notes.
- Amendments and retractions that preserve event history.
- Day summaries, record history lookup, filtered queries, and task trees.
- Archive integrity validation.
- JSON export of current visible records.
- Legacy v1 archive migration into the v2 store.
- Explicit destructive truncation with timestamped backup.
- Git-backed sync of the authoritative event stream as one compressed artifact.
- Backfilled timestamps and timezone-controlled day attribution.
- Runtime work-context capture including cwd and Git metadata when available.

## Command Reference

Global options:

```bash
sillok --store /path/to/sillok.db <command>
sillok --human <command>
sillok --json <command>
sillok --at 2026-05-13T10:00:00Z <command>
sillok --tz America/Denver <command>
```

Global option details:

- `--store`: use a specific store path; defaults to `SILLOK_STORE` or XDG data
  storage.
- `--human`: print verbose readable summaries instead of quiet default output.
- `--json`: print the verbose JSON response envelope.
- `--at`: assign the event timestamp. Accepts RFC3339 or naive
  `YYYY-MM-DDTHH:MM:SS`.
- `--tz`: timezone for local day attribution and naive `--at` parsing.
- `SILLOK_ACTOR`: optional actor label recorded on new events; defaults to
  `agent`.

Initialize the archive if absent:

```bash
sillok init
```

Record a task or work note. Notes default to `completed`; use `--status` for
`open`, `active`, `blocked`, or `completed`:

```bash
sillok note "Implemented timerange query indexing" --tags rust,sillok
sillok note "Investigating archive compaction" --status active --purpose "Reduce read latency"
sillok note "Split reducer from view indexing" --parent <record_id> --tags rust,indexing
```

`note` creates a task event. Without `--parent`, Sillok opens or reuses the day
record for the event timestamp and links the task under that day. With
`--parent`, the task is linked under an existing active record.

The status vocabulary is `open`, `active`, `blocked`, `completed`, and
`retracted`. Prefer `sillok retract` when hiding a record so the retraction
reason is preserved.

Manage day objectives:

```bash
sillok objective add "Finish archive indexing"
sillok objective add "Finish archive indexing" --tags rust,storage
sillok objective complete <objective_id> --note "All scoped work is complete"
```

Objectives are day-scoped records. Completing an objective changes its derived
status to `completed`; an optional completion note is stored on the record.

Amend current derived state:

```bash
sillok amend <record_id> --text "Corrected task text"
sillok amend <record_id> --status completed
sillok amend <record_id> --purpose "Clarify why this work mattered"
sillok amend <record_id> --tags rust,indexing
```

`amend` records a new event and updates only the supplied fields in the current
projection. It requires at least one changed field.

Retract a task or objective from current views:

```bash
sillok retract <record_id> --reason "Recorded against the wrong objective"
```

Retraction hides a task or objective from normal current views while preserving
the underlying event history. Day records cannot be retracted.

Read records:

```bash
sillok show <record_id>
sillok day
sillok day --date 2026-05-13
sillok query --from 2026-05-13T00:00:00 --to 2026-05-13T23:59:59
sillok query --from 2026-05-13T00:00:00 --to 2026-05-13T23:59:59 --tag rust
sillok query --from 2026-05-13T00:00:00 --to 2026-05-13T23:59:59 --status completed
sillok query --from 2026-05-13T00:00:00 --to 2026-05-13T23:59:59 --context /home/cyh/project
sillok tree --root <record_id>
sillok tree --date 2026-05-13
```

Read command behavior:

- `show` returns one current record plus every event that references it.
- `day` returns the selected day's tree, objectives, and visible non-day
  records. Omit `--date` to use the current day in the selected timezone.
- `query` returns visible current records created in an inclusive time range.
  Filter by `--context`, `--tag`, or `--status`.
- `tree` renders the visible record tree rooted at `--root`, or at the selected
  day when `--date` is supplied.

Validate and export:

```bash
sillok doctor
sillok export json
sillok export json --from 2026-05-13T00:00:00 --to 2026-05-13T23:59:59
```

`doctor` validates SQLite integrity and replays persisted events to confirm the
derived projection. `export json` returns visible current records; with
`--from` and `--to`, export is limited to records created in that inclusive
range.

Configure and run Git-backed event archive sync. Sync stores one
`bitcode` + zstd level 22 archive artifact in the configured Git remote; the
SQLite/Turso database remains local and is rebuilt from events when needed:

```bash
sillok sync remote set git@github.com:you/sillok-archive.git
sillok sync remote set /path/to/archive.git --branch main --path sillok.slk.zst
sillok sync remote show
sillok sync pull
sillok sync push
sillok sync run
```

Sync command behavior:

- `sync remote set <URL>` writes `<store>.sync.json`; defaults are branch
  `main` and artifact path `sillok.slk.zst`.
- `sync remote show` prints the configured URL, branch, artifact path, and
  sidecar path.
- `sync pull` **overwrites** the local database with the remote archive
  (adopting its `archive_id`), keeping a timestamped backup of the old database.
  When the remote artifact is absent it is a no-op and leaves local state alone.
- `sync push` **overwrites** the remote artifact with the local archive. With no
  local archive it fails with `archive_missing`.
- `sync run` is the merge path: it merges by `event_id` when both sides share an
  `archive_id`, rebuilds the local database with a backup, and pushes. When the
  `archive_id`s differ it cannot merge — an interactive terminal is prompted to
  keep the local side (push) or the remote side (pull), while non-interactive
  runs (`--json` or no TTY) refuse with `sync_mismatch_needs_choice`.

Because each `init` mints its own `archive_id`, two independently-initialized
stores cannot merge directly. Bootstrap a second machine with `sync pull` once
(it adopts the remote `archive_id`); afterward both sides share an id and
`sync run` merges normally. Git authentication is delegated to the user's
existing `git` setup.

Migrate a legacy v1 archive:

```bash
sillok --store /path/to/sillok.slk.zst migrate --dry-run
sillok --store /path/to/sillok.slk.zst migrate --target /path/to/sillok.db --yes
```

`migrate --dry-run` validates the source archive and reports the migration plan.
Writing a v2 target requires `--yes`. If `--target` is omitted, Sillok writes
`sillok.db` beside the legacy archive.

Reset the archive only when an operator explicitly asks for a full reset. This
creates a timestamped backup first:

```bash
sillok truncate --yes
```

`truncate` is destructive and requires `--yes`. It backs up the current v2
database, removes it, then initializes a fresh archive.

Backfill with explicit timestamps and timezone attribution:

```bash
sillok --tz Asia/Seoul --at 2026-05-13T21:30:00 note "Backfilled a late task"
sillok --at 2026-05-13T21:30:00+09:00 note "Backfilled a Seoul-time task"
```

RFC3339 timestamps keep their explicit offset. Naive timestamps are interpreted
in the selected `--tz`, or in the system-local timezone when `--tz` is omitted.

## Agent Integration

Add a short Sillok section to a repository `AGENTS.md` so coding agents record
objectives and completed work while they operate:

````markdown
## Sillok

Use Sillok for substantive agentic work. Record objectives, completed tasks,
and corrections during the session instead of relying on chat history. Quiet
output is the default for agents; use `--json` only when IDs or the full
response envelope are needed, and `--human` only for summaries intended for a
person.
Never run `sillok truncate --yes` unless the user explicitly asks to reset the
whole archive.

```bash
sillok objective add "Ship the storage/indexing refactor"
sillok note "Split reducer from view indexing" --parent <objective_id> --tags rust,sillok
sillok amend <record_id> --status completed
sillok note "Fixed relink regression coverage" --parent <objective_id> --tags tests
sillok show <record_id>
sillok query --from 2026-05-13T00:00:00 --to 2026-05-13T23:59:59 --tag rust
sillok objective complete <objective_id> --note "Scoped work is complete"
sillok day --human
```

Use `--at` for backfilled work and `--tz` when day attribution matters:

```bash
sillok --tz America/Denver --at 2026-05-13T16:45:00 note "Backfilled release notes" --tags docs
```
````

## Development

Run checks:

```bash
cargo fmt
cargo clippy --all-targets --all-features
cargo test
```

Project constraints:

- No `unwrap()` or `expect()` in project source or tests.
- Keep modules under 300 lines where practical.
- Keep folder `mod.rs` files to module declarations only.
- Prefer explicit error handling and structured tracing.
- Do not treat the live database or legacy archive serialization as a public
  standard; use CLI export/migration commands for compatibility.

## Design Notes

See [docs/plan/sillok-cli-chronicle-design.md](docs/plan/sillok-cli-chronicle-design.md)
for the initial implementation plan and data model notes. See
[docs/architecture/be/turso-store.md](docs/architecture/be/turso-store.md),
[docs/architecture/be/view-indexing.md](docs/architecture/be/view-indexing.md),
and [docs/architecture/be/git-sync.md](docs/architecture/be/git-sync.md) for
backend architecture notes.
