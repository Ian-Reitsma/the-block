# SimpleDb – Snapshot-Oriented Key-Value Store

`SimpleDb` provides a lightweight, in-memory map with crash-safe snapshot
persistence for tests and feature-gated builds that do not link RocksDB. The
implementation lives in [`node/src/simple_db/memory.rs`](../node/src/simple_db/memory.rs)
and is used by the peer cache, DNS record store, DEX order book, law-enforcement
portal logs, and other subsystems that require deterministic persistence without
external dependencies.

## 1. Column-Family Snapshots

Each column family (CF) is serialized with `bincode` and written to disk as a
single file named `<base64(cf)>.bin`, where the identifier is encoded with
`URL_SAFE_NO_PAD` base64. Examples:

| Column family | On-disk file |
| --- | --- |
| `default` | `ZGVmYXVsdA.bin` |
| `governance:fee_floor` | `Z292ZXJuYW5jZTpmZWVfZmxvb3I.bin` |
| `dex:escrow` | `ZGV4OmVzY3Jvdy.bin` |

The loader performs a strict round-trip check (`decode → encode == original`)
before accepting a filename as base64. Sanitized legacy dumps that happen to be
valid base64 strings therefore fall back to underscore decoding rather than
being misclassified. When both a base64 and an underscore-sanitized file exist
for the same column family, the base64 snapshot wins and the legacy file is
deleted once the new image is parsed.

Legacy layouts from earlier releases stored snapshots as `<cf_with_underscores>.bin`.
The loader continues to recognise those names by replacing `_` with `:` after
failing the base64 round-trip check. If a matching base64 file is present, the
legacy snapshot is ignored and removed to prevent stale data from shadowing the
latest state.

## 2. Crash-Safe Rewrite Path

`SimpleDb` writes every mutation through an atomic, fsync-backed staging
process:

1. Serialize the updated map into a `NamedTempFile` allocated inside the target
directory.
2. `write_all` the bytes, then `sync_all` the temporary file to ensure the
filesystem persists the payload.
3. If an existing base64 snapshot is present, rename it to `<name>.bin.old` as a
one-deep backup.
4. `persist` the temporary file into place. On success, remove the backup (if
any) and delete the legacy underscore-named file. On failure, restore the backup
so callers can retry without losing state.

The helper delays in-memory mutations until persistence succeeds. If `persist`
or `sync_all` fails, the map rolls back to the previous value before returning
the error. Empty column families remove both the base64 file and any lingering
legacy snapshot. The optional byte limit set via `set_byte_limit()` guards
against runaway allocations prior to serialization.

`SimpleDb::default()` now retains ownership of the temporary directory that it
creates, ensuring the backing path remains live for the lifetime of the handle.
Use `SimpleDb::open(path)` when you want snapshots in a specific directory.

## 3. API Highlights

- `try_insert_cf(name, key, value)` – upserts a namespaced key and returns the
  previous value if present.
- `insert_cf_with_delta(name, items)` – applies a batch of mutations and returns
  a rollback delta vector that can be replayed on error.
- `put_cf_raw` / `delete_cf_raw` – byte-oriented helpers for subsystems that
  already own serialized payloads.
- `flush_wal()` – a no-op shim for compatibility with RocksDB-backed call sites
  that expect explicit flush hooks.

## 4. Usage Example

```rust
use the_block::simple_db::SimpleDb;

let mut db = SimpleDb::open("/tmp/simpledb-demo");
db.try_insert_cf("dex:escrow", "job-1", b"locked".to_vec()).unwrap();
assert_eq!(db.get("job-1"), Some(b"locked".to_vec()));
```

Column families are created lazily; the `default` CF always exists and backs the
`get` / `insert` convenience methods.

## 5. Regression Coverage

- `node/src/simple_db/memory.rs` contains unit tests that prove legacy
  underscore snapshots load correctly, base64 names round-trip, and persisted
  data survives reopen.
- `node/tests/simple_db/memory_tests.rs::loads_legacy_sanitized_cf_files` keeps
  the backward-compatibility path covered.
- `node/tests/simple_db/memory_tests.rs::prefers_base64_snapshots_over_legacy`
  verifies the precedence rule and clean-up behaviour.
- `node/tests/simple_db/memory_tests.rs::cf_names_are_base64_encoded` confirms
  every emitted filename uses the base64 convention.

To run just the lightweight backend regressions during development, target the
library tests directly:

```bash
cargo test -p the_block --lib simple_db::memory_tests::tests::prefers_base64_snapshots_over_legacy -- --nocapture
```

## 6. Operational Guidance

- Keep snapshots on SSD-backed paths; every write fsyncs the temporary file
  before promotion.
- Monitor disk quotas alongside the optional byte limit to avoid failed writes
  during stress tests.
- When migrating from legacy underscores to base64, allow the loader to clean up
  superseded files automatically rather than deleting them manually.
- The lightweight backend is intended for tests and constrained environments.
  Production deployments should continue to use the RocksDB backend under
  `--features storage-rocksdb`.
