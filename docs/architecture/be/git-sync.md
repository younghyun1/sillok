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

`sync run`:

1. Export the local event archive from SQLite when present.
2. Prepare a temporary Git worktree and fetch the configured branch when present.
3. Decode the remote artifact when present.
4. Merge by `event_id` for matching `archive_id` values.
5. Topologically order events so record creation precedes later mutations.
6. Validate the merged archive with `ChronicleView`.
7. Atomically rebuild the local SQLite store from the merged archive, with a timestamped backup.
8. Encode the merged archive and push it to the remote.
9. On push rejection, fetch, re-merge, retry once, then fail with `sync_push_rejected`.

Archive ID mismatches fail with `sync_archive_mismatch` and do not replace either side.

## Error Codes

- `sync_remote_missing`: no sidecar config exists for this store
- `sync_archive_mismatch`: local and remote non-empty archives have different archive IDs
- `sync_merge_conflict`: event IDs or dependencies conflict during merge
- `sync_git_error`: a Git command other than push failed
- `sync_push_rejected`: push was rejected after retry handling
