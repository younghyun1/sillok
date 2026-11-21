# Changelog

## 1.2.0

### Added

- **Hooks.** `sillok hook` handles Claude Code and Codex hook events, and `sillok skill install --hooks` registers it for both. At session start it returns the repository's status as context, so resuming costs no tool call, and on Claude Code it exports `SILLOK_SESSION` so events carry the session label. At the end of a turn that changed files or systems without a Sillok write, it sends the turn back once with a reminder. Registration is idempotent, keeps every other setting, and replaces entries for the standalone `sillok-reminder` binary, whose logic now lives here.
- **Id prefixes.** Any command that takes an id accepts a prefix of at least eight hex digits that names one record; a shared prefix fails with `ambiguous_id`.
- **Batch notes.** `sillok note --stdin` records one task per JSON line in a single transaction, with the command's flags as defaults for lines that omit a field.
- **`doctor --prune`.** `doctor` now reports the backup files beside the store and leftover sync directories under `leftovers`; `--prune` deletes them.

### Changed

- **`status` matches a repository across URL spellings.** `git@github.com:o/r.git` on one machine and `https://github.com/o/r` on another are the same repository; 1.1 compared the URLs exactly.
- **`objective list` keeps the newest 20** by default instead of 50. `--limit` is unchanged.
- **Completing an objective starts a background sync** when one is due, so finished work does not wait for the next session.

### Compatibility

The store, event format, and sync layout are unchanged.

## 1.1.1

### Added

- **Background sync.** With a sync remote configured, `sillok status` starts `sillok sync` as a detached process when the last attempt is more than 60 minutes old, and returns without waiting. The result is kept in `<store>.sync.state.json`; a failure appears in the next `status` under `warnings`, and `sync remote show` prints the last result. `sillok sync auto <minutes>` sets the interval, and `0` or `SILLOK_AUTOSYNC=0` turns it off. Before this, a store was only as current as the last manual `sync`.

### Performance

- **System allocator.** jemalloc is gone on every platform: a command lives for about a millisecond, so the allocator's startup work cost more than it returned.
- **Logging is set up on demand.** The JSON log subscriber was built on every invocation although only migration and sync cleanup log. `SILLOK_LOG` still installs it at startup.
- **One fewer connection per sync.** Sync ran `ls-remote` and then `fetch` before any push. It now fetches first and recognises a branch that does not exist yet from the failure, falling back to `ls-remote` only for an unfamiliar message.

### Compatibility

The remote layout, event format, and store are unchanged, and 1.0 and 1.1.0 replicas sync with 1.1.1 as before. The sync sidecar gains an optional `auto_minutes` that older binaries ignore.

## 1.1.0

### Added

- **The agent skill ships in the binary.** `sillok skill install` writes the workflow as `sillok/SKILL.md` for Claude Code (`~/.claude/skills`) and Codex (`~/.codex/skills`, or `~/.agents/skills` when that is the root in use), or into any skill root with `--dir`. It is idempotent, replaces an existing copy, and keeps one copy per tool: a second copy Codex would also discover is named in a warning instead of being updated. `skill status` reports each copy as `current`, `outdated`, `newer`, `modified`, `unmanaged`, or `missing`; `skill print` prints the file.
- **Installed skills follow upgrades.** Each installed file is stamped with the version and a digest. `sillok status` replaces a user-level copy that an older Sillok wrote and nobody edited, and says so once in `warnings`; hand-edited copies are reported and left alone, and files Sillok did not write are never touched. `SILLOK_SKILL_AUTOUPDATE=0` disables this.
- `status` output gains `remote` when the repository has an `origin`.

### Changed

- **`status` covers the whole repository.** It matched the working tree's root path, so it came back empty inside a linked Git worktree and never showed work synced from another machine. It now also matches the main checkout of a linked worktree and every checkout with the same `origin` URL.
- `sillok guide` and the skill are one text. It now states the record-before-the-turn-ends rule, the `correction` tag, and closing what you open.

### Fixed

- An exported but empty `SILLOK_STORE` was a usage error; it now means the default store, like the other variables.
- `status` dropped its warnings when the store did not exist yet.

### Performance

- Every command opened the database twice, once only to look for a 0.x layout, and `objective complete` opened it four times. One connection now classifies the file with a single catalog query.
- Repository matching for `status` uses a new expression index on the context's remote URL. Stores from 1.0 gain it on first open through an additive `schema_rev` in `store_meta`; no migration runs, and a 1.0 binary still opens the store.

### Dependencies

- rusqlite 0.40 to 0.43 (bundled SQLite), clap 4.6.9, uuid 1.29, zstd 0.14.4, and refreshed transitive dependencies.

## 1.0.2

### Fixed

- Windows builds use the system allocator instead of compiling jemalloc, whose MSVC build requires Unix tools and is untested by the Rust wrapper. Linux and macOS keep jemalloc.
- The legacy sync test serializes Windows paths as valid JSON.

### Dependencies

- Upgraded dependencies with `cargo upgrade --incompatible` and refreshed transitive dependencies, including serde_json, uuid, zstd, and build dependencies.
- Replaced the property-test adapter with native Proptest generators because the adapter does not compile with current Proptest. Replay-order and merge-split checks keep their coverage and gain structured input shrinking.

## 1.0.1

### Fixed

- `sillok sync` failed with `pathspec 'sillok.slk.zst' did not match any files` whenever it had events to push after an earlier sync had already removed the 0.10 artifact, which is the normal state for every machine upgraded after the first. The removal is now staged with `git rm --cached --ignore-unmatch`, which is a no-op when the file is gone. Workaround on 1.0.0: `sillok sync remote set <url>` rewrites the config without `legacy_path`.

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
