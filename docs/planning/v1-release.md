# 1.0 release plan

## Why

0.10 was built for one agent at a time. Under parallel agents Turso's exclusive file lock made 28 of 32 concurrent writes and 12 of 16 concurrent reads fail. Every sync after a local write rebuilt the database and left a full backup behind, peaking near 830 MB of memory because zstd level 22 streamed without a known input size. Events were bitcode, which cannot evolve without breaking older replicas. Tasks belonged to exactly one Day record keyed by date plus a timezone label, so work could not span days and `--tz` split days in two.

## Decisions

- **Derived days.** No Day records; a day is a query over event times in the requested zone. Tasks and objectives span days naturally.
- **JSON events with canonical bytes.** Written once and never re-serialized, so unknown fields and types survive older readers and sync can compare bytes.
- **rusqlite in WAL mode** instead of Turso: concurrent readers, waiting writers, no async runtime.
- **Git-friendly sync.** Manifest plus monthly JSONL files, uncompressed, rewritten only when they change.
- **Agent-first CLI.** Bare ids from writes, compact JSON reads, JSON errors on stderr, `status` for resuming, `guide` for onboarding; old spellings kept as hidden aliases.
- **Compatibility bar.** 0.9/0.10 stores, archives, and sync artifacts import automatically and deterministically; minor semantic differences are accepted and documented in the README.

## Verification

- Golden fixtures written by the 0.10 code (`tests/fixtures/v0_10`) cover every v2 event kind; the v2 store, v1 archive, and v2 sync artifact all convert to byte-identical events.
- A copy of a real 0.10 store (5,609 events, WAL included) migrated in 1.4 s with every visible record matching 0.10 field for field.
- 32 parallel writers and 16 parallel readers all succeed.
- Property tests: replay ignores arrival order; merging any split of an event set restores the whole.
- Dev-build benchmarks against 0.10 are recorded in `CHANGELOG.md`.

## Not in 1.0

- Prebuilt binaries: `.cargo/config.toml` targets the host CPU, so release artifacts would need a baseline `target-cpu`.
- Idempotency keys for retried writes.
