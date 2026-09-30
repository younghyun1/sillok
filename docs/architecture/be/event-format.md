# Event format

The chronicle is a set of immutable events. Everything else (records, day views, trees) is derived from them, so the event format is the part of Sillok that must stay readable forever. It is covered by semantic versioning.

## Envelope

One event is one JSON object, serialized once when it is created:

```json
{"event_id":"01a0f03f-0164-76b7-9651-b9d0a43f5d36","event_at":"2026-09-30T02:57:36.099Z","recorded_at":"2026-09-30T02:57:36.099Z","actor":"agent","context":{"cwd":"/repo/src","git_root":"/repo","git_branch":"main","git_head":"3b2c398c...","git_remote":"https://github.com/o/r.git","session":"s-42"},"kind":{"type":"task_recorded","record_id":"01a0f03f-0163-778a-9ecb-1b15640507a2","parent_id":"01a0f03f-015e-70a1-97a8-1466a44a84dd","text":"Split reducer","tags":["rust"],"status":"completed"}}
```

- `event_id`, `record_id`, `parent_id`: UUIDv7 strings.
- `event_at`: when the work happened; `--at` backfills set it. Day views use it.
- `recorded_at`: when the event was written. Last-writer-wins ordering and sync month buckets use it.
- Timestamps are RFC 3339 UTC with millisecond precision and a `Z` suffix.
- Absent optional fields are omitted, not written as `null` (except `record_moved.parent_id`, where `null` means top-level).

## Canonical bytes

The bytes written at creation are canonical. The store keeps them in `event.event_json`, sync writes them verbatim as one line of a month file, and no version ever re-serializes a stored event. That is what lets an older binary carry fields and types a newer one added without dropping them, and it is how sync detects divergence: the same `event_id` with different bytes. When that happens every replica keeps the lexicographically smaller bytes, so they converge without human input.

## Types

| `type` | Fields | Effect |
| --- | --- | --- |
| `objective_added` | `record_id`, `parent_id?`, `text`, `tags?`, `status` | Creates an objective |
| `task_recorded` | `record_id`, `parent_id?`, `text`, `purpose?`, `tags?`, `status` | Creates a task |
| `record_amended` | `record_id`, `text?`, `status?`, `purpose?`, `clear_purpose?`, `tags?`, `note?` | Changes given fields; `tags: []` clears |
| `record_moved` | `record_id`, `parent_id` | Re-parents; `null` is top-level |
| `record_retracted` | `record_id`, `reason` | Hides; remembers the prior status |
| `record_restored` | `record_id` | Unhides with the prior status |

## Replay

Replay depends only on the set of events, never on arrival order:

1. Creation events build records; if one record id is created twice, the earliest `(recorded_at, event_id)` wins.
2. Parents that do not exist, and creation-time cycles, become top-level (replay reports a note).
3. Other events apply in `(recorded_at, event_id)` order; a move that would create a cycle or targets a missing parent is skipped.
4. Unknown types are counted and skipped.

The live write path applies the same rules incrementally (`domain::reducer::rules`); `sillok doctor` replays every event and compares the result field by field with the stored projection.

## Evolution rules

These keep 1.x readers compatible with archives written by later 1.x versions:

1. New fields must be optional and must not change the meaning of existing fields. Readers ignore unknown fields.
2. New behavior gets a new `type` name. Existing types are never renamed, removed, or repurposed.
3. A change older readers must not silently skip (for example, a type that alters how existing records replay) raises the sync manifest's `min_reader_version`, so older binaries refuse the remote with `unsupported_format` instead of diverging.
4. Timestamp and id formats never change.

## Legacy data

0.9 and 0.10 stored bitcode-encoded events (in the Turso store's `events.event_payload` and in `.slk.zst` archives). `src/legacy` holds frozen copies of those types and converts them deterministically; the golden fixtures in `tests/fixtures/v0_10` pin the conversion. Conversion drops archive and day markers, maps parents that were days to `None`, turns `ObjectiveCompleted` into a `record_amended` with `status: completed` and `note`, and turns an amend to `retracted` (or a task created as `retracted`) into a `record_retracted` with a fixed reason and an id derived with UUIDv5, so every machine derives the same bytes.
