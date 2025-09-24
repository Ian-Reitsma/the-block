# On-Chain Blob Storage Flow
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

This document describes the end‑to‑end lifecycle of a file that is uploaded
through the The‑Block storage interface and committed on chain as a `BlobTx`.
It is intentionally verbose so future auditors can follow each step without
referencing external context.

## 0. Storage Engine Abstraction

Persistent components in the node now route all key-value access through the
`crates/storage_engine` crate. This crate defines a `KeyValue` trait with
associated batching, iterators, and telemetry methods so RocksDB, sled, and the
in-memory engine expose a single API surface. Every subsystem opens its
database via `SimpleDb::open_named(<name>, <path>)`, which looks up the desired
backend in `[storage]` within `config/default.toml`. Operators can override the
`default_engine` or specific handles such as `gossip_relay`,
`net_peer_chunks`, and `gateway_dns` to pin light-weight stores to sled or the
in-memory backend while keeping the ledger on RocksDB. The storage engine layer
also surfaces health metrics (`storage_engine_*` gauges) so dashboards can track
pending compactions, SST growth, and memtable pressure regardless of the chosen
backend. Introducing a new engine requires implementing the trait and hooking it
into the registry; no call sites outside `SimpleDb` need to change.

## 1. Local Chunking & Hashing

1. A user invokes `wallet blob-put <file> <owner>`.
2. The wallet opens a local `StoragePipeline` (default `./blobstore`) and
   registers a dummy provider for demonstration purposes.
3. The file is chunked into ~1 MiB pieces; each chunk is encrypted with
   ChaCha20‑Poly1305 and encoded using a Local Reconstruction Code with
   \((k,r,h)=(16,5,3)\), then overlaid with a Progressive Fountain layer
   \((k'=2,n'=5)\) so single‑shard losses repair over BLE.
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