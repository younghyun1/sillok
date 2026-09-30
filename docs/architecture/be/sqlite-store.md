# SQLite store

The local store is one SQLite database (bundled through `rusqlite`), default `$XDG_DATA_HOME/sillok/sillok.db`. It holds the authoritative event log plus projections that make reads cheap.

## Concurrency

Agents run Sillok in parallel, so the store is opened in WAL mode with `busy_timeout` of 10 seconds: readers never block, and a second writer waits for the first instead of failing. Every write is one `BEGIN IMMEDIATE` transaction that validates, appends the event, and updates the projection, so a check and its write cannot interleave with another process. `synchronous=FULL` because a chronicle entry that returned an id must survive power loss. There is no process-wide lock file; the only lock file is `<store>.migrate.lock`, held while a 0.x store is migrated.

## Schema (store version 3)

Tables are `STRICT`, columns carry their table's name as a prefix, and constraints are named. Text bounds in `CHECK` constraints mirror `domain::text` and a unit test keeps them in agreement.

| Table | Role |
| --- | --- |
| `store_meta` | `store_version`, `archive_id`, `created_at_ms` |
| `event` | The log: `event_id`, `event_kind`, `event_record_id`, occurred/recorded ms, `event_work_context_id`, `event_json` (canonical bytes) |
| `work_context` | Deduplicated contexts; the JSON plus indexed `cwd`, `git_root`, `session` |
| `record` | Current state per record (`WITHOUT ROWID`), including `record_prior_status` for restore |
| `record_tag` | One row per tag, cascading from `record` |

`event_json` is a deliberate exception to normalization: those bytes are the archive's source of truth and what sync compares. Every other table is a projection; `rebuild` deletes and replays them inside one transaction (no file swap, no backup), which `sillok doctor --repair` and sync use.

Indexes follow the queries: events by occurred time (day windows), by record (history), by recorded time, and by context (status); records by parent (trees), by kind and status (objective lists), by created and updated time; tags by text. `work_context` rows are insert-only, so its foreign keys need no delete-side index; `record_work_context_idx` exists for the status query.

## Opening

`Store::open` creates a missing store only for writes; reads against a missing store return empty results without creating a file. An existing file is classified before use: store version 3 opens directly, a 0.9/0.10 store (`sillok_meta.store_datashape_version = 2`) is migrated first, and anything else fails with `unsupported_datashape`.

## Migration from 0.9/0.10

1. Take `<store>.migrate.lock` (bounded wait) and re-check the version, so concurrent first runs migrate once.
2. Read every v2 event through SQLite, including events still in the Turso-written WAL, and convert them (see `event-format.md`).
3. Build the v3 database at `<store>.v3-<ms>.tmp`, replay the projection, checkpoint, and close it.
4. Back up the old store with `VACUUM INTO <store>.v2-<ms>.bak.db` (one self-contained file), remove the old `-wal`/`-shm`, and rename the new file into place.

A crash before the rename leaves the old store untouched; the temporary file is removed on failure. Recovery after an unwanted migration: rename the `.v2-*.bak.db` back to `sillok.db` and run a 0.10 binary.

## Reset

`sillok reset --yes` writes a `VACUUM INTO` backup, then deletes every row and assigns a new archive id in one transaction, so concurrent readers never see a missing file.
