# 0.10 golden fixtures

Written by `examples/golden_fixtures.rs` at the 0.10 codebase (commit that adds this directory). They freeze every shape a 0.9/0.10 install could persist so the 1.0 importer can be tested against real bytes:

- `store.db`: v2 SQLite store (Turso-written, checkpointed).
- `archive.slk.zst`: v1 legacy archive (bitcode + zstd level 3).
- `sync.slk.zst`: v2 sync artifact (bitcode + zstd level 22).
- `expected.json`: the source events and the records 0.10 considered visible.

Do not regenerate them with a later version; the generator only compiles against the 0.10 API.
