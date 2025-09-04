# SimpleDb – WAL-Backed Key-Value Store

`SimpleDb` is a lightweight embedded database used by several subsystems
(net peer cache, DNS records, chunk gossip, Dex order books, and more).
It trades advanced features for a compact, auditable write-ahead log (WAL)
and deterministic serialization. This guide explains the on-disk layout,
quota mechanics, and recovery process.

## 1. On-Disk Layout

`node/src/simple_db.rs` stores two files under the supplied path:

- `db` – bincode-serialized `HashMap<String, Vec<u8>>` containing the
  current key/value state and a special `__wal_id` entry tracking the last
  applied WAL record.
- `wal` – append-only log of pending changes. Each entry is a
  `WalEntry { op, checksum }` where `op` is either `WalOp::Record` or
  `WalOp::End { last_id }`. Entries are serialized with bincode and
  checksummed with BLAKE3 to detect corruption.

On startup `SimpleDb::open()` loads `db` and then replays any `wal` entries
newer than `__wal_id`. A terminal `WalOp::End` indicates that the previous
session flushed successfully and the WAL can be discarded.

## 2. WAL Write Path

Mutation methods (`try_insert`, `try_remove`) first append a `WalOp::Record`
with a monotonically increasing `id`. The record contains the key, optional
value, and the sequence number. Only after the WAL write succeeds does the
in-memory map update and the `__wal_id` key advance. `try_flush()` rewrites
`db`, appends a terminal `WalOp::End`, and deletes the WAL.

## 3. Byte Quotas and Disk-Full Handling

`SimpleDb` exposes `set_byte_limit()` to cap the serialized `db` size. When
the limit would be exceeded, `try_flush()` returns `io::ErrorKind::Other`
and metrics `STORAGE_DISK_FULL_TOTAL` increments under the `telemetry`
feature. Callers should surface this failure and either purge unneeded keys
or back off retries.

## 4. Corruption Recovery

During `open()` each WAL entry’s checksum is verified. Corrupt entries are
skipped and `WAL_CORRUPT_RECOVERY_TOTAL` increments. Because every record is
id-stamped, replays are idempotent: re-applying a record with an older `id`
has no effect. If the WAL ends without a terminal `End` entry, `open()`
replays what it can and then removes the WAL so the database continues with
best-effort recovery.

## 5. Usage Examples

```rust
use the_block::SimpleDb;

let mut db = SimpleDb::open("/tmp/example");
db.try_insert("key", b"value".to_vec()).unwrap();
assert_eq!(db.get("key"), Some(b"value".to_vec()));
```

Modules relying on `SimpleDb` include:

- `node/src/net/peer.rs` – chunk gossip cache.
- `node/src/gateway/dns.rs` – published DNS TXT records.
- `node/src/dex/storage.rs` – persistent DEX order books.
- `node/src/identity/handle_registry.rs` – username-to-key mapping.

## 6. Testing and Fuzzing

- `node/tests/wal_recovery.rs` exercises crash recovery and byte limits.
- `fuzz/fuzz_targets/wal_fuzz.rs` mutates WAL bytes to stress checksum and
  replay paths. `make fuzz-wal` runs this harness nightly.

## 7. Operational Guidelines

- Place databases on fast SSDs; WAL flushes are synchronous.
- Monitor disk usage and the `STORAGE_DISK_FULL_TOTAL` counter.
- For unit tests, use temporary directories to avoid polluting the
  repository tree.

SimpleDb is intentionally minimal. Complex query patterns should be modeled
as higher-level indices built on top of this primitive rather than
embedding additional features into the core store.
