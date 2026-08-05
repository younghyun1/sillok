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

`sync` (explicitly `sync run`) is the only data operation; it meshes both sides so neither is discarded:

1. Export the local archive and decode the remote artifact. When both are missing, fail with `archive_missing`. When only one side has an archive, the other adopts it unchanged.
2. Merge by `event_id` across both archives, including archives that never shared an `archive_id`. Every event from either side survives; the only refusal is the same `event_id` carrying different payloads, which fails with `sync_merge_conflict`.
3. Topologically order events so record creation precedes later mutations, validate with `ChronicleView`, rebuild the local store (timestamped backup), and push. The push is skipped when the artifact bytes already match the remote head.
4. On push rejection (the remote advanced concurrently), re-merge on the new head and retry once, then fail with `sync_push_rejected`.

Two invariants make cross-archive meshing safe:

- **Deterministic identity.** Each `init` mints a random `archive_id`, so independently-initialized machines start from different archives. When they mesh, the identity ordered first by `(created_at, archive_id)` survives. The choice is a pure function of the two inputs, so every replica converges on a byte-identical artifact instead of ping-ponging ids through the remote.
- **Day canonicalization.** Both machines can independently open the same calendar day under different `day_id`s. All `DayOpened` events survive the merge untouched, but the view reducer keeps the first one per day key as canonical and treats later ones as aliases, resolving every event reference through the alias map. Records from both sides therefore land under one Day record per date, and events are never rewritten, which keeps merges pure unions.

## Error Codes

- `sync_remote_missing`: no sidecar config exists for this store
- `archive_missing`: local and remote archives are both missing
- `sync_merge_conflict`: one `event_id` carries different payloads, or event dependencies cannot be ordered
- `sync_git_error`: a Git command other than push failed
- `sync_push_rejected`: push was rejected after retry handling
