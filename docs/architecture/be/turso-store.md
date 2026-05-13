# Turso Store

Sillok v2 uses a local Turso/SQLite database as the live store. The event log
remains authoritative, while current records are materialized into indexed
projection tables for command latency.

## Datashape Versions

- `PRAGMA user_version = 2`
- `sillok_meta.store_datashape_version = 2`
- `events.event_datashape_version = 2`
- `records.record_datashape_version = 2`

Unsupported versions must fail with `unsupported_datashape` instead of trying
to coerce unknown rows.

## Tables

- `events`: append-only event order, timestamps, actor, context, primary record,
  and bitcode-encoded `EventKind`.
- `event_refs`: record ids touched by each event, used by `show`.
- `records`: current derived state by record id.
- `record_tags`: normalized tag index.
- `days`: timezone-specific day key to day id.
- `work_contexts`: deduplicated captured working context.
- `sillok_meta`: archive id, creation timestamp, and store version.

## Command Paths

Normal mutations should:

1. Open a transaction.
2. Validate only the required parent/day/record rows.
3. Insert one event.
4. Update affected projection rows and indexes.
5. Commit.

Normal reads should use projection tables and indexes. `doctor` is allowed to
replay all events and compare the replayed `ChronicleView` with projections.

## Migration

Legacy v1 `.slk.zst` archives remain private. `sillok migrate --yes` decodes the
archive, validates it with `ChronicleView`, copies the source to a timestamped
backup, imports events/projections into a temporary v2 database, validates the
database, and renames it into place.
