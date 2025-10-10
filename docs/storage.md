# On-Chain Blob Storage Flow
> **Review (2025-09-30):** Documented the in-house LSM engine plus Reed–Solomon/LT coding defaults and tuning knobs.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This document describes the end‑to‑end lifecycle of a file that is uploaded
through the The‑Block storage interface and committed on chain as a `BlobTx`.
It is intentionally verbose so future auditors can follow each step without
referencing external context.

## 0. Storage Engine Abstraction

Persistent components in the node now route all key-value access through the
`crates/storage_engine` crate. The crate defines a `KeyValue` trait with
batched writes, prefix iterators, and telemetry hooks so every backend exposes a
uniform surface. `SimpleDb::open_named(<name>, <path>)` looks up the configured
engine in `[storage]` within `config/default.toml` and defaults to the
in-house LSM implementation. Operators can still override specific handles such
as `gossip_relay`, `net_peer_chunks`, and `gateway_dns` to force an in-memory
store for integration tests. The storage engine layer reports
`storage_engine_*` gauges so dashboards can track memtable pressure, SST growth,
and compaction activity regardless of the configured backend. Introducing a new
engine requires implementing the trait and registering it inside `SimpleDb`;
call sites outside the module stay unchanged.

> **2025-10 update.** The workspace now ships a first-party `sled` crate that
> reuses the `storage_engine::inhouse_engine` backend while keeping the familiar
> `Config`/`Db`/`Tree` APIs. Existing directories produced by the third-party
> crate are auto-migrated when the optional `legacy-format` feature is enabled;
> otherwise the opener emits a clear error instructing operators to rebuild with
> the flag or run `tools/storage_migrate` ahead of time.

> **2025-11 update.** The first-party serialization stack now handles state
> snapshots, contract stores, schema markers, and audit appenders directly.
> These flows emit deterministic binary blobs or escaped JSON without touching
> `serde`. The remaining crates keep targeting the
> `foundation_serialization` helpers, which still return `Err(Unimplemented)`
> until their respective codecs are ported.

### 0.1 In-house LSM tree

- **Memtable + WAL.** Each column family keeps a BTreeMap memtable mirrored to a
  JSONL write-ahead log. Updates allocate a monotonically increasing sequence
  number so crash recovery can replay operations deterministically.
- **SSTables.** When the memtable crosses the configured byte limit (default
  8 MiB) it is written to an immutable SST file under
  `<db>/<cf>/sst-<id>.bin`. Files store `(key, sequence, value|tombstone)`
  tuples serialized with the shared binary facade.
- **Compaction.** Background compaction rewrites all SSTs for a column family
  into a single sorted file, dropping superseded entries and persisted
  tombstones. Every compaction resets the WAL and advances the manifest’s file
  id counter.
- **Column families.** Each CF lives under its own directory with an isolated
  WAL and manifest (`manifest.json`). `SimpleDb::ensure_cf` guarantees the
  directory exists before use, so higher-level code keeps the legacy
  `ensure_cf` semantics without special cases.
- **Tuning knobs.** `SimpleDb::set_byte_limit` delegates to the engine’s
  `set_byte_limit` to adjust the per-CF memtable ceiling. Operators can push the
  limit up for throughput-heavy stores or shrink it on low-memory hosts.

### 0.2 Migration helper

`tools/storage_migrate` reads legacy RocksDB or sled directories and rewrites
their column families into the in-house format (the same format used by the
first-party `sled` crate). The tool walks each CF, streams entries into the new
engine, and writes tombstones for missing keys so deletes are preserved. Invoke
it with:

```
cargo run -p storage-migrate -- <legacy-path> <inhouse-path>
```

The command is idempotent—re-running the migration compacts existing SSTs and
replays the WAL so operators can stage migrations safely before switching the
node configuration.

## 1. Local Chunking & Hashing

1. A user invokes `wallet blob-put <file> <owner>`.
2. The wallet opens a local `StoragePipeline` (default `./blobstore`) and
   registers a dummy provider for demonstration purposes.
3. The file is chunked into ~1 MiB pieces; each chunk is encrypted with
   ChaCha20‑Poly1305 and encoded using the in-house Reed–Solomon coder with
   \((k=16, p=8)\), then overlaid with three deterministic LT fountain shards
   so single‑shard losses repair over BLE.
4. Every shard is stored locally and referenced in an `ObjectManifest`. The
   manifest itself is hashed with BLAKE3 to produce a deterministic
   `manifest_hash`.
5. Independently, a BLAKE3 hash over the raw file bytes becomes the
   `blob_root` of the emitted `BlobTx`. A random 32‑byte `blob_id` is assigned.
6. Rent escrow is charged at `rent_rate_ct_per_byte × file_size` and locked
   against the uploader's account.
7. The CLI prints the `blob_root` for reference. In a full deployment the
   returned `BlobTx` would be signed and submitted via the `submit_tx` RPC.

## 2. Block Inclusion

1. Storage providers collect pending `BlobTx` transactions and store the
   corresponding shards. Each blob is identified solely by its root hash, so
   nodes may fetch shards lazily.
2. The next Layer‑2 scheduler slot (4‑second cadence) aggregates blob roots into
   a Merkle tree and anchors its root in the L1 block header via the
   `l2_roots`/`l2_sizes` fields.
3. During block validation every `l2_root` must have at least *k* available
   shards within the data‑availability window or the block is rejected.

## 3. Retrieval

1. A client wishing to retrieve a file issues `wallet blob-get <blob_id> <out>`.
2. The wallet decodes the blob identifier, fetches the corresponding manifest
   from the local store, and downloads the referenced shards. In this demo the
   shards are already present locally; a production node would fetch them from
   peers.
3. The shards are reassembled using Reed‑Solomon decoding and decrypted back to
   the original bytes. The result is written to `<out>`.
4. Reads incur no user fees. When the pipeline reconstructs data it increments
   the `SUBSIDY_BYTES_TOTAL{type="read"}` counter so gateways can later claim
   `READ_SUB_CT` inflation rewards.

## 4. Rent Escrow & Expiry

1. Upon `BlobTx` submission, the storage pipeline locks `rent_rate_ct_per_byte`
   multiplied by `blob_size` from the uploader's CT balance.
2. When the blob is explicitly deleted or its expiry epoch is reached, 90 % of
   the escrowed CT is refunded to the original depositor and 10 % is burned.
3. Long‑tail audits may challenge providers up to 180 days later; failure to
   supply a shard results in a slashing penalty.

This document will evolve as the blob pipeline is fully wired into the network
stack and wallet UX. Every step above corresponds to a discrete code module so
engineers can trace the flow from CLI invocation to on-chain commitment.
