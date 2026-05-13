# View Indexing

Sillok persists an append-only archive and derives command output through an
in-memory `ChronicleView`. The archive format is private; performance work
should prefer projection/index changes before changing serialized events.

## Index Contract

- `records` is the authoritative derived record map.
- `children` and `parent_by_child` must be updated together during reduction.
- `day_by_key` maps a timezone-specific day key to its day record.
- `by_day` maps a day record id to all records attributed to that day.
- `timeline` maps creation timestamps to record ids for range queries.
- `by_tag`, `by_context`, and `by_status` are secondary filters.

All vector index buckets are sorted by `(created_at, record_id)` after rebuild.
Command handlers can rely on deterministic output without rescanning or sorting
the full record map.

## Mutation Rules

Reducers should mutate only primary state while applying events. After all
events are applied, rebuild secondary indexes in one pass and validate the
parent graph. Relinking a child must remove it from only the previous parent
bucket recorded in `parent_by_child`; scanning every parent bucket should be
reserved for repair tooling, not normal reduction.

## Query Rules

Range queries should walk the smallest available exact index before falling
back to `timeline`. Context matching intentionally remains substring-based, so
the context index is for exact-key future use and export diagnostics rather than
partial matching acceleration.
