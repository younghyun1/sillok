# Git Sync Implementation Plan

## Objective

Add a Git-backed sync command group that stores Sillok's authoritative event archive as a single max-compressed `bitcode` plus `zstd` artifact in a Git remote. The local SQLite/Turso database remains the live projection store.

## Implemented Scope

- Add `sillok sync remote set <URL> [--branch main] [--path sillok.slk.zst]`
- Add `sillok sync remote show`
- Add `sillok sync pull`
- Add `sillok sync push`
- Add `sillok sync run`
- Persist per-store remote config at `<store>.sync.json`
- Share archive encode/decode helpers while preserving legacy zstd level 3 writes
- Encode sync artifacts at zstd level 22
- Use the system `git` binary; do not add `git2`
- Merge matching archives by `event_id`
- Reject non-empty archive ID mismatches
- Rebuild local SQLite atomically from merged archives with backups
- Retry push rejection once after re-fetching and re-merging

## Verification

- Unit coverage for sync archive codec roundtrip
- Unit coverage for merge deduplication, divergent merges, dependency ordering, and archive ID mismatch
- CLI integration coverage using local bare Git remotes for remote config, push, pull, run merge, and mismatch preservation
- Run `cargo fmt`, `cargo clippy --all-targets --all-features`, and `cargo test`
