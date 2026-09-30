# Changelog

## 1.0.0

1.0 rebuilds Sillok for agents running in parallel and for a chronicle that has to stay readable for years. Existing 0.9/0.10 stores and sync remotes migrate automatically.

### Breaking

- **Days are derived, not stored.** `day` shows every record with an event that day, with per-record activity and ancestors; tasks and objectives span days. Day records and `day_id` are gone from output.
- **Output shapes.** Writes print the bare affected id; reads print compact JSON without empty fields or work context (`--full` adds it); errors are JSON on stderr, with exit code 2 for usage errors.
- **Event format.** Events are canonical JSON (see `docs/architecture/be/event-format.md`); bitcode is used only to import 0.x data.
- **Sync layout.** The remote holds `sillok/manifest.json` plus monthly `sillok/events/YYYY-MM.jsonl` instead of one `sillok.slk.zst`. A 0.10 binary cannot read a 1.0 store or remote; upgrade every machine.
- **Renamed commands**, with the old spellings kept as hidden aliases: `migrate` is `import`, `truncate` is `reset`, `export json` is `export`, `tree --root <id>` is `tree <id>`.
- `note`/`amend` reject `--status retracted`; use `retract`, which records a reason. Objectives may only nest under objectives.

### Added

- `status` (open objectives and recent work for the current repository), `objective list`, `move`, `restore`, `guide`, `import`, `sync --dry-run`, `doctor --repair`.
- `query` gains `--since`, `--date`, repeatable `--tag`, `--kind`, `--open`, `--text`, and `--limit`; `--from`/`--to` are optional.
- `amend` gains `--clear-purpose`, `--clear-tags`, and `--note`.
- `SILLOK_TZ`, `SILLOK_SESSION`, `SILLOK_OUTPUT`, and `SILLOK_LOG`.

### Fixed

- Concurrent commands no longer fail: 0.10 lost 28 of 32 parallel writes and 12 of 16 parallel reads to Turso's exclusive file lock. The store is now SQLite in WAL mode with a busy timeout.
- Sync no longer rebuilds and backs up the whole database after every local write, and no longer leaves unbounded `.bak.db` files. A sync after one new note dropped from 42.5 s at 825 MB RSS to 0.26 s at 41 MB (dev builds).
- `amend --status retracted` could retract Day records and create reasonless retractions.
- Days no longer split by timezone label (`--tz America/Denver day` found nothing recorded without `--tz`).
- Recorded Git remotes no longer keep embedded `user:token@` credentials.
- `doctor` compares every record field against a replay instead of only counting rows.

### Performance

Dev build against dev build on a 5,600-event store: `day` 32.8 ms to 11.6 ms, a month query filtered by tag 3.0 s to 13 ms, `note` 26.9 ms to 4.7 ms. Git context is captured only for writes, with two parallel `git` processes instead of four sequential ones. tokio is gone; jemalloc is the allocator.

### Migration notes

The first command after upgrading converts the store in place and keeps `sillok.db.v2-<ms>.bak.db`. On the real 0.10 store used for testing, 5,609 events became 5,485 (124 archive and day markers dropped) and all 4,400 visible records matched 0.10 field for field. Accepted differences: records directly under a day become top-level, a task recorded under an earlier day's parent shows on its own day, and objective completion notes move from `purpose` to `note`.
