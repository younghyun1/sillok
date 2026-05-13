# Sillok Agent Chronicle CLI

## Summary

Sillok is a Rust CLI for agentic daily work logging. It records natural-language work notes, objectives, amendments, and retractions into one user-global archive while exposing deterministic JSON for autonomous harnesses.

The default store is `$XDG_DATA_HOME/sillok/sillok.slk.zst`, falling back to `~/.local/share/sillok/sillok.slk.zst`. `SILLOK_STORE` or `--store` overrides the path. The serialized format is private; the current implementation stores append-only events as bitcode compressed with zstd.

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
- `sillok truncate --yes` backs up the whole archive and starts over.

JSON is the default output. `--human` is for interactive summaries.

## Data Model

The archive is an append-only event stream. Every archive, event, day, task, and objective id is UUIDv7. The current state is derived from events at load time.

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

## In-Memory Indexing

On every command, Sillok decodes the archive and builds a `ChronicleView` optimized for command execution:

- `HashMap<ChronicleId, DerivedRecord>` for current state.
- `HashMap<ChronicleId, Vec<ChronicleId>>` for parent-to-children trees.
- `HashMap<ChronicleId, ChronicleId>` for child-to-parent lookup.
- `HashMap<DayKey, ChronicleId>` for day lookup.
- `BTreeMap<Timestamp, Vec<ChronicleId>>` for timerange queries.
- Secondary indexes for tags, context keys, and status.

Derived output remains sorted by timestamp and id for deterministic JSON.

## Storage Safety

Mutations acquire an exclusive file lock, load and validate the archive, append events, then write via a same-directory temp file and atomic rename. Read commands use shared locks where practical. `truncate --yes` copies the old archive to a timestamped backup before writing a fresh initialized archive.

## Validation And Tests

The implementation validates non-empty text, tag normalization, missing parent references, retracted parent use, parent cycles, timestamp ranges, and invalid destructive commands.

Coverage includes archive roundtrips, day auto-open behavior, parent-child derivation, retraction filtering, query filtering, JSON command shape, truncate backups, and `doctor` validation.
