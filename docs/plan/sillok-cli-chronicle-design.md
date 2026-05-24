# Sillok Agent Chronicle CLI

## Summary

Sillok is a Rust CLI for agentic daily work logging. It records natural-language work notes, objectives, amendments, and retractions into one user-global local store while keeping successful write output silent by default and exposing deterministic JSON for autonomous harnesses when requested.

The default store is `$XDG_DATA_HOME/sillok/sillok.db`, falling back to `~/.local/share/sillok/sillok.db`. `SILLOK_STORE` or `--store` overrides the path. The live v2 datashape is a private Turso/SQLite database. Legacy v1 archives at `sillok.slk.zst` remain readable by `doctor`, `export`, and `migrate`.

## Command Contract

- `sillok init` initializes the archive if absent.
- `sillok note <TEXT>` records a completed task under the current local day, or under `--parent ID`.
- `sillok objective add <TEXT>` adds an open day objective.
- `sillok objective complete <ID>` marks an objective complete.
- `sillok amend <ID>` appends a corrective event for text, status, purpose, or tags.
- `sillok retract <ID> --reason TEXT` tombstones a task or objective from current views.
- `sillok show <ID>` returns current state plus event history.
- `sillok day [--date YYYY-MM-DD]` returns the day master task, objectives, records, and tree.
- `sillok query --from TIME --to TIME` returns records created in an inclusive timerange.
- `sillok tree [--date YYYY-MM-DD] [--root ID]` returns a derived parent-child tree.
- `sillok doctor` validates archive decode, schema, references, and parent cycles.
- `sillok export json` returns current visible records.
- `sillok migrate --store LEGACY.slk.zst --target sillok.db --yes` migrates a v1 archive into the v2 store.
- `sillok truncate --yes` backs up the whole archive and starts over.

Successful write/action commands are silent by default. Read commands return
compact JSON data. `--json` returns the verbose JSON envelope, and `--human` is
for interactive summaries.

## Data Model

The event stream remains the source of truth. Every archive, event, day, task, and objective id is UUIDv7. In v2, current state is materialized into indexed SQL projection tables instead of being derived from every event on each command.

Event kinds:

- `ArchiveInitialized`
- `DayOpened`
- `ObjectiveAdded`
- `ObjectiveCompleted`
- `TaskRecorded`
- `TaskAmended`
- `TaskRetracted`
- `TaskLinked`
- `TaskUnlinked`

The local calendar day is a master day record. First write for a day appends `DayOpened`; notes and objectives attach under that day unless an explicit parent is supplied.

## Turso Store Indexing

The v2 database stores append-only events and indexed current projections:

- `events` stores event order, event metadata, and bitcode event payloads.
- `records` stores current derived state by record id.
- `days` maps timezone-specific day keys to day records.
- `record_tags` supports tag filtering.
- `event_refs` supports `show` event history without scanning all events.
- `work_contexts` deduplicates captured working context.

Derived output remains sorted by timestamp and id for deterministic JSON.

## Storage Safety

Mutations run in a database transaction, insert one append-only event, and update only affected projection rows. `doctor` can replay all events into `ChronicleView` to verify projection consistency. `truncate --yes` copies the old database to a timestamped backup before writing a fresh initialized store. `migrate --yes` copies the legacy archive to a timestamped backup, builds a temporary database, validates it, then atomically renames it into place.

## Validation And Tests

The implementation validates non-empty text, tag normalization, missing parent references, retracted parent use, parent cycles, timestamp ranges, and invalid destructive commands.

Coverage includes archive migration, day auto-open behavior, parent-child derivation, retraction filtering, query filtering, JSON command shape, truncate backups, and `doctor` validation.
