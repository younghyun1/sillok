# Git Sync Backend

Sillok sync stores the authoritative append-only event archive as one Git-tracked artifact. The live Turso/SQLite database remains a local projection cache: normal commands read and mutate SQLite, while sync exports events, merges archives, and rebuilds projections locally.

## Artifact

- Default branch: `main`
- Default path: `sillok.slk.zst`
- Encoding: `Archive` encoded with `bitcode`, then zstd-compressed at level `22`
- Commit message: `sync: update sillok archive`

The artifact is intentionally not encrypted. Git remote access controls and the user's existing Git authentication handle transport security.

## Configuration

Each store has a sidecar file at `<store>.sync.json`:

```json
{
  "schema_version": 1,
  "url": "/path/or/url/to/remote.git",
  "branch": "main",
  "path": "sillok.slk.zst"
}
```

The artifact path must be relative and stay inside the temporary Git worktree.

## Operation

Sync uses the system `git` binary through `std::process::Command`. This keeps SSH keys, credential helpers, and user Git configuration delegated to the existing environment and avoids a native Git library dependency.

Each store is initialized with its own random `archive_id`, so two machines that both run `init` start from different archives. `pull` and `push` therefore **overwrite** by default and ignore `archive_id`; `run` is the merge path and only merges archives that already share an `archive_id`.

`sync pull` — the remote archive overwrites the local store:

1. Export the local archive (for the summary and backup) and prepare a worktree.
2. Decode the remote artifact. When it is absent, this is a no-op: the local store is left untouched and no error is raised.
3. Atomically rebuild the local SQLite store from the remote archive, with a timestamped backup. The local store adopts the remote `archive_id`.

`sync push` — the local archive overwrites the remote:

1. Export the local archive. When it is absent, fail with `archive_missing`.
2. Prepare a worktree on the fetched remote branch head, write the local archive over the artifact, commit on top, and push. The push is always a fast-forward, so no force is needed; the artifact content is fully replaced while remote history is kept.
3. On push rejection (the remote advanced concurrently), rebuild on the new head and retry once, then fail with `sync_push_rejected`.

`sync run` — bidirectional merge for stores that share an `archive_id`:

1. Export the local archive and decode the remote artifact.
2. When both sides share an `archive_id`, merge by `event_id`, topologically order events so record creation precedes later mutations, validate with `ChronicleView`, rebuild the local store (timestamped backup), and push.
3. When the `archive_id`s differ, the archives cannot be merged. Interactive terminals are prompted to keep the local side (push) or the remote side (pull). Non-interactive runs (`--json`, or no TTY on stdin) refuse with `sync_mismatch_needs_choice` rather than guessing.
4. On push rejection, retry once, then fail with `sync_push_rejected`.

Bootstrap path between two independently-initialized stores: run `sync pull` once so the local store adopts the remote `archive_id`; from then on both sides share an id and `sync run` merges normally.

## Error Codes

- `sync_remote_missing`: no sidecar config exists for this store
- `archive_missing`: `push` (or `run`) found no local archive to upload
- `sync_mismatch_needs_choice`: `run` hit differing `archive_id`s and cannot prompt (use `pull` or `push`)
- `sync_aborted`: the user declined the `run` mismatch prompt
- `sync_archive_mismatch`: emitted only by the underlying merge when archives differ (reached via the unit-tested merge path)
- `sync_merge_conflict`: event IDs or dependencies conflict during merge
- `sync_git_error`: a Git command other than push failed
- `sync_push_rejected`: push was rejected after retry handling
