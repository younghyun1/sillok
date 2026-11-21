# Git sync

Sync shares the event log between machines through any Git remote the user can push to. Git authentication, transport, and history are delegated to the system `git`; the SQLite store never leaves the machine.

## Layout

```text
<dir>/manifest.json          {"format":"sillok-archive","format_version":1,"min_reader_version":1,"archive_id":...,"created_at":...}
<dir>/events/YYYY-MM.jsonl   canonical event lines, bucketed by the UTC month of recorded_at
```

`<dir>` defaults to `sillok`. Lines are sorted by `(recorded_at, event_id)`. Because `recorded_at` is the write time, new events land at the end of the current month's file: a sync commit is usually a few appended lines, old months never change, and Git's delta compression keeps the repository small. Files are uncompressed text so `git log -p` reads like a journal.

## Configuration

`<store>.sync.json`:

```json
{"schema_version": 2, "url": "git@github.com:you/archive.git", "branch": "main", "dir": "sillok"}
```

An optional `auto_minutes` sets the background interval (see below); binaries before 1.1.1 ignore it. A 0.10 sidecar (`schema_version: 1` with `path`) is upgraded on first read; its `path` becomes `legacy_path`. The URL and branch may not start with `-`, the branch may not contain whitespace or `:`, and `dir`/`legacy_path` must be relative paths inside the repository, so config values can never be read as Git options or escape the worktree.

## One attempt

1. Read local event ids, recorded times, and bytes.
2. Create a temporary repository, shallow-fetch the branch, and read the layout (bounded: 1 MiB per line, 1 GiB total). A manifest with `min_reader_version` above this build's reader version stops sync with `unsupported_format`.
3. If `legacy_path` exists in the checkout, decode and convert the 0.10 artifact.
4. Plan the pull with `domain::merge::plan`: remote and legacy events the local store lacks, plus conflicting ids whose remote bytes are smaller.
5. Apply the pull in one transaction (insert events, rebuild the projection). Nothing is rebuilt when nothing arrived.
6. Mark a month dirty when any event in it differs from the remote; rewrite only dirty months, the manifest (the older `(created_at, archive_id)` identity wins, so replicas agree), and delete the legacy artifact.
7. Commit (`sync: +N events (YYYY-MM)`) and push. Nothing is committed when nothing changed.

The fetch in step 2 is the only connection before the push. It names the branch, so on an empty remote or a first sync it fails with Git's `couldn't find remote ref`, which is read as "no branch yet" and the attempt continues on a new one. Any other fetch failure is checked with `ls-remote`: no such head means the same thing, anything else is `sync_git_error`. An unfamiliar message therefore costs one more round trip rather than a wrong answer.

A rejected push (the remote moved) is retryable: up to 3 attempts with exponential backoff and jitter, each starting from step 1. `--dry-run` stops after step 6 and reports counts.

## Background sync

Sync is only useful when it runs, so `status`, the command every session starts with, starts one when it is due. With a remote configured, background sync not disabled, and the last attempt older than the interval (60 minutes unless `auto_minutes` says otherwise; `sillok sync auto <minutes>` sets it and `0` turns it off, as does `SILLOK_AUTOSYNC=0`), `status` stamps the attempt in `<store>.sync.state.json` and spawns `sillok sync --background` in its own process group with no inherited streams, then returns without waiting. Stamping first means sessions that open together start one sync, and the interval also spaces retries after a failure.

The child runs an ordinary sync and writes its result to the state file: `finished_at`, `ok`, `pulled`, `pushed`, or `error` as `code: message`. The next `status` puts a failed result in `warnings`; `sync remote show` prints the state. A child killed before it reports, for example by a sandbox that ends with its command, leaves only the attempt stamp and is retried after the interval. Background and manual syncs may overlap: both go through the same transaction and push-rejection retry.

## Non-interactive Git

Every Git command runs with `LC_ALL=C` (failures are classified by Git's English messages), `GIT_TERMINAL_PROMPT=0` and, unless the user set `GIT_SSH_COMMAND`, `ssh -o BatchMode=yes`, so missing credentials fail with `sync_git_error` instead of waiting for input an agent cannot give. Commits disable signing and set a fixed `sillok` identity.

## Error codes

- `sync_remote_missing`: no sidecar
- `sync_config_error`: invalid sidecar values
- `sync_git_error`: a Git command other than push failed (auth, network)
- `sync_push_rejected`: the remote kept moving through every retry
- `unsupported_format`: the remote was written by a newer, incompatible Sillok
