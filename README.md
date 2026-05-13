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

## Output

JSON is the default output and is intended for agents:

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

JSON records include stable RFC3339 `created_at` and `updated_at` fields. Use
`--human` for interactive summaries with local-device timestamps rendered as
readable wall-clock time:

```bash
sillok --human day
```

## Command Reference

Global options:

```bash
sillok --store /path/to/sillok.db <command>
sillok --human <command>
sillok --json <command>
sillok --at 2026-05-13T10:00:00Z <command>
sillok --tz America/Denver <command>
```

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

Manage day objectives:

```bash
sillok objective add "Finish archive indexing"
sillok objective add "Finish archive indexing" --tags rust,storage
sillok objective complete <objective_id> --note "All scoped work is complete"
```

Amend current derived state:

```bash
sillok amend <record_id> --text "Corrected task text"
sillok amend <record_id> --status completed
sillok amend <record_id> --purpose "Clarify why this work mattered"
sillok amend <record_id> --tags rust,indexing
```

Retract a task or objective from current views:

```bash
sillok retract <record_id> --reason "Recorded against the wrong objective"
```

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

Validate and export:

```bash
sillok doctor
sillok export json
sillok export json --from 2026-05-13T00:00:00 --to 2026-05-13T23:59:59
```

Migrate a legacy v1 archive:

```bash
sillok --store /path/to/sillok.slk.zst migrate --dry-run
sillok --store /path/to/sillok.slk.zst migrate --target /path/to/sillok.db --yes
```

Reset the archive only when an operator explicitly asks for a full reset. This
creates a timestamped backup first:

```bash
sillok truncate --yes
```

Backfill with explicit timestamps and timezone attribution:

```bash
sillok --tz Asia/Seoul --at 2026-05-13T21:30:00 note "Backfilled a late task"
sillok --at 2026-05-13T21:30:00+09:00 note "Backfilled a Seoul-time task"
```

## Agent Integration

Add a short Sillok section to a repository `AGENTS.md` so coding agents record
objectives and completed work while they operate:

````markdown
## Sillok

Use Sillok for substantive agentic work. Record objectives, completed tasks,
and corrections during the session instead of relying on chat history. JSON is
the default output for agents; use `--human` only for summaries intended for a
person. Never run `sillok truncate --yes` unless the user explicitly asks to
reset the whole archive.

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
for the initial implementation plan and data model notes.
