# Storage Erasure Coding and Reconstruction

The storage pipeline protects blobs with local-reconstruction codes (LRC) and a
small fountain overlay so nodes can recover data even when shards are missing.
Implementation lives in `node/src/storage/erasure.rs`.

## Parameters

The encoder splits each chunk into **K = 16** data shards and adds **R = 5**
local parities plus **H = 3** global parities for a total of **24** Reed–Solomon
shards. A fountain overlay then contributes **3** additional XOR shards derived
from the first two data shards, bringing the per-chunk total to **27**. The
constants are defined at the top of the module:
[`node/src/storage/erasure.rs`](../node/src/storage/erasure.rs#L3-L32).

```text
K (data) = 16
R (local parity) = 5
H (global parity) = 3
PARITY = R + H = 8
```

After Reed–Solomon encoding, a lightweight fountain overlay combines the first
two shards to produce three additional XOR-based shards. These appended shards
help long-range repairs by providing extra combinations beyond the RS parity
set and can substitute for either of the first two data shards during
reconstruction.

## Encoding

`encode` pads the chunk to `ceil(len / K)` per-shard size, fills the data shards,
appends parity placeholders, and invokes `ReedSolomon::encode`. The function
returns the RS-coded shards with the fountain shards appended.

```rust
use node::storage::erasure;
let shards = erasure::encode(&blob)?; // Vec<Vec<u8>> of length 24+
```

## Reconstruction

`reconstruct` accepts a vector of optional shards (including fountain shards)
and the expected ciphertext length of the chunk. Reed–Solomon fills in missing
entries, optionally using the fountain overlay to recover either of the first
two data shards. All recovered data shards are concatenated and trimmed to the
requested length, yielding the original encrypted chunk.

```rust
let original = erasure::reconstruct(shards, chunk_cipher_len)?;
```

## Integration

- `pipeline.rs` calls `encode` when ingesting blobs and `reconstruct` when peers
  supply partial data during repair cycles. `get_object` now assembles all
  required shards per chunk (data, parity, and fountain overlay) and trims the
  decrypted plaintext to the manifest’s recorded length.
- `repair.rs` performs on-demand reconstruction to heal missing shards, then
  re-encodes the chunk so each missing shard (data, parity, and overlay) is
  rewritten with the correct bytes.
- Network messages carry individual shards via the `Shard` variant so gossip
  traffic can rebalance storage.

## Tuning and Operations

1. **Shard Counts** – Adjusting `K`, `R`, or `H` requires network-wide
   coordination because shard indices are baked into manifests. The current
   16/5/3 split balances overhead (~50%) with recovery capability.
2. **Fountain Overlay** – The `overlay_fountain` helper adds three extra XOR
   shards from the first two data shards, providing cheap diversity for
   opportunistic repairs and a limited ability to recover the first or second
   shard directly from the overlay.
3. **Verification** – Reconstruction failures bubble up as `Err(String)`; the
   caller should treat them as potential corruption and re-fetch the blob.
4. **Testing** – `StoragePipeline` unit tests simulate random shard loss and
   ensure the original data is recoverable when at least `K` shards are present
   and that the repair loop rewrites missing shards with fresh encodings.
5. **Metrics** – `storage_repair_bytes_total` and `storage_chunk_put_seconds`
   expose encoding and repair costs; monitor these to spot hotspots.

For a walkthrough of the entire storage pipeline, see
[`docs/storage_pipeline.md`](storage_pipeline.md).

## Repair Loop Operations and Observability

The periodic repair worker now executes chunk reconstruction on a bounded
thread pool (`MAX_CONCURRENT_REPAIRS` = 4) so that a burst of degraded manifests
cannot monopolise CPU time. Each shard recovery attempt is recorded to disk
under `storage/repair_log/` as line-delimited JSON containing the manifest
fingerprint, chunk index, status (`success`, `failure`, `skipped`, or `fatal`),
bytes written, and any error cause. Logs rotate daily and the directory retains
the 14 most recent files.

Repair attempts and failures feed Prometheus metrics that power dashboards:

- `storage_repair_attempts_total{status="success|failure|skipped|fatal"}`
  increments for every attempt outcome.
- `storage_repair_failures_total{error="manifest|integrity|reconstruct|encode|database"}`
  captures structured failure causes so operators can spot corruption versus
  transient capacity issues.
- `storage_repair_bytes_total` continues to report the volume of data
  reconstructed.

Clients can introspect the repair loop via the storage RPC and CLI tooling:

- `storage.repair_history` returns recent log entries (limit configurable).
- `storage.repair_run` triggers a one-shot repair sweep and returns a summary
  of manifests touched and bytes restored.
- `storage.repair_chunk` forces a repair attempt for a specific manifest hash
  and chunk index, optionally rewriting all shards when `--force` is set.
- The CLI mirrors these endpoints via `tb storage repair-history`,
  `tb storage repair-run`, and `tb storage repair-chunk`.

The Explorer’s storage view exposes the same repair history with timestamped
status, byte counts, and error text so operators can audit long-running
clusters. Backoff state is persisted per `(manifest, chunk)` and surfaced as
`status="skipped"` entries containing the next retry time, ensuring repeated
failures do not thrash the encoder.
