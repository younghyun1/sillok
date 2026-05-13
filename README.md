# Sillok

Sillok is a Rust CLI for meticulous agentic work chronicles. It records daily
objectives, completed tasks, corrections, and retractions into a structured
archive instead of a fragile append-only text log.

The name comes from Korean `sillok`, the court chronicles of Joseon. The design
goal is similar: keep precise records of what happened, when, under what
objective, and in what working context.

## Status

Sillok is an early local-first CLI. The command interface is intended to be
stable enough for agent harnesses, but the serialized archive format is private
and may change.

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

By default, Sillok stores one user-global archive at:

```text
$XDG_DATA_HOME/sillok/sillok.slk.zst
```

If `XDG_DATA_HOME` is unset, it falls back to:

```text
~/.local/share/sillok/sillok.slk.zst
```

Override the store with either:

```bash
sillok --store /path/to/sillok.slk.zst day
SILLOK_STORE=/path/to/sillok.slk.zst sillok day
```

The current implementation persists a compressed private archive. On every
mutation, it loads the archive, builds indexed in-memory state, validates the
change, appends events, and atomically rewrites the archive.

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

## Daily Workflow

Initialize explicitly, or let the first write initialize the archive:

```bash
sillok init
```

Add an objective for the day:

```bash
sillok objective add "Finish archive indexing"
```

Record a completed task:

```bash
sillok note "Implemented timerange query indexing" --tags rust,sillok
```

Attach a note under an objective by passing the objective id as the parent:

```bash
sillok note "Split reducer from view indexing" --parent <objective_id> --tags rust,sillok
```

Complete the objective:

```bash
sillok objective complete <objective_id> --note "All scoped work is complete"
```

Read today:

```bash
sillok day
```

Read a specific day:

```bash
sillok day --date 2026-05-13
```

Query a timerange:

```bash
sillok query --from 2026-05-13T00:00:00 --to 2026-05-13T23:59:59
```

## Agent Integration

Add a short Sillok section to a repository `AGENTS.md` so coding agents record
objectives and completed work while they operate:

````markdown
## Sillok

Use Sillok for substantive agentic work. Record objectives, completed tasks,
and corrections during the session instead of relying on chat history.

```bash
sillok objective add "Ship the storage/indexing refactor"
sillok note "Split reducer from view indexing" --parent <objective_id> --tags rust,sillok
sillok note "Fixed relink regression coverage" --parent <objective_id> --tags tests
sillok objective complete <objective_id> --note "Scoped work is complete"
sillok day --human
```

JSON is the default and is intended for agents. Use `--human` when presenting a
summary to a person.
````

## Corrections

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

Show current state and event history for one record:

```bash
sillok show <record_id>
```

Render a tree:

```bash
sillok tree --root <record_id>
sillok tree --date 2026-05-13
```

## Backfill And Timezones

Use `--at` to record when work occurred, and `--tz` to choose day attribution
for naive timestamps:

```bash
sillok --tz Asia/Seoul --at 2026-05-13T21:30:00 note "Backfilled a late task"
```

RFC3339 timestamps keep their offset:

```bash
sillok --at 2026-05-13T21:30:00+09:00 note "Backfilled a Seoul-time task"
```

## Maintenance

Validate the archive:

```bash
sillok doctor
```

Export current records as JSON:

```bash
sillok export json
sillok export json --from 2026-05-13T00:00:00 --to 2026-05-13T23:59:59
```

Reset the whole archive. This creates a timestamped backup first:

```bash
sillok truncate --yes
```

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
- Do not treat the serialized archive as a public standard; optimize the
  in-memory representation and CLI behavior first.

## Design Notes

See [docs/plan/sillok-cli-chronicle-design.md](docs/plan/sillok-cli-chronicle-design.md)
for the initial implementation plan and data model notes.
