# Storage Erasure Coding and Reconstruction

The storage pipeline protects blobs with local-reconstruction codes (LRC) and a
small fountain overlay so nodes can recover data even when shards are missing.
Implementation lives in `node/src/storage/erasure.rs`.

## Parameters

The encoder splits each chunk into **K = 16** data shards and adds **R = 5**
local parities plus **H = 3** global parities for a total of **24** shards.  The
constants are defined at the top of the module:
[`node/src/storage/erasure.rs`](../node/src/storage/erasure.rs#L3-L7).

```text
K (data) = 16
R (local parity) = 5
H (global parity) = 3
PARITY = R + H = 8
```

After Reed–Solomon encoding, a lightweight fountain overlay combines the first
two shards to produce additional XOR-based shards. These appended shards help
long-range repairs by providing extra combinations beyond the RS parity set.

## Encoding

`encode` pads the chunk to `ceil(len / K)` per-shard size, fills the data shards,
appends parity placeholders, and invokes `ReedSolomon::encode`. The function
returns the RS-coded shards with the fountain shards appended.

```rust
use node::storage::erasure;
let shards = erasure::encode(&blob)?; // Vec<Vec<u8>> of length 24+
```

## Reconstruction

`reconstruct` accepts a vector of optional shards where missing entries are
`None`. Reed–Solomon fills in the gaps and the first data shard is returned as
the reassembled chunk.  The helper is used by the repair logic when peers supply
subset shards.

```rust
let original = erasure::reconstruct(shards)?;
```

## Integration

- `pipeline.rs` calls `encode` when ingesting blobs and `reconstruct` when peers
  supply partial data during repair cycles.
- `repair.rs` performs on-demand reconstruction to heal missing shards.
- Network messages carry individual shards via the `Shard` variant so gossip
  traffic can rebalance storage.

## Tuning and Operations

1. **Shard Counts** – Adjusting `K`, `R`, or `H` requires network-wide
   coordination because shard indices are baked into manifests. The current
   16/5/3 split balances overhead (~50%) with recovery capability.
2. **Fountain Overlay** – The `overlay_fountain` helper adds five extra XOR
   shards from the first two data shards, providing cheap diversity for
   opportunistic repairs.
3. **Verification** – Reconstruction failures bubble up as `Err(String)`; the
   caller should treat them as potential corruption and re-fetch the blob.
4. **Testing** – `node/tests/storage_erasure.rs` simulates random shard loss and
   ensures the original data is recoverable when at least `K` shards are
   present.
5. **Metrics** – `storage_repair_bytes_total` and `storage_chunk_put_seconds`
   expose encoding and repair costs; monitor these to spot hotspots.

For a walkthrough of the entire storage pipeline, see
[`docs/storage_pipeline.md`](storage_pipeline.md).
