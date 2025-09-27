# Storage Erasure Coding and Reconstruction
> **Review (2025-09-25):** Synced Storage Erasure Coding and Reconstruction guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The storage pipeline protects blobs with local-reconstruction codes (LRC) and a
small fountain overlay so nodes can recover data even when shards are missing.
Implementation lives in `node/src/storage/erasure.rs`.

## Parameters

The encoder splits each chunk into **K = 16** data shards and adds **R = 5**
local parities plus **H = 3** global parities for a total of **24** Reed–Solomon
shards. A fountain overlay then contributes **3** additional XOR shards derived
from the first two data shards, bringing the per-chunk total to **27**. The
defaults live in [`config/storage.toml`](../config/storage.toml) and load
through [`coding::Config`](../crates/coding/src/config.rs) so operators can tune
the ladder without patching source. The current constants are defined at the top
of [`node/src/storage/erasure.rs`](../node/src/storage/erasure.rs#L3-L32).

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

### XOR fallback parity

Operators that need to avoid third-party erasure crates can switch the coding
config to `erasure.algorithm = "xor"`, trading efficiency for supply-chain
independence. The XOR coder keeps the same shard layout but produces a single
parity shard that is just the XOR of all data shards (duplicated when the
manifest expects multiple parities). The repair loop recognises the active
algorithm and skips jobs that would require more than one missing data shard or
that lack any surviving parity, logging
`algorithm_limited:<alg>:missing=<n>:parity=<m>` entries in the repair log. This
ensures operators get actionable diagnostics instead of opaque reconstruction
failures when running the fallback path.

### Fallback rollout controls

Fallback components are gated in `config/storage.toml` under the `[rollout]`
section:

```toml
[rollout]
allow_fallback_coder = false
allow_fallback_compressor = false
require_emergency_switch = false
# emergency_switch_env = "TB_STORAGE_FALLBACK_EMERGENCY"
```

- `allow_fallback_coder` / `allow_fallback_compressor` must be set to `true`
  before `erasure.algorithm = "xor"` or `compression.algorithm = "rle"` will be
  accepted at start-up.
- When `require_emergency_switch = true`, fallback algorithms remain disabled
  until the environment variable referenced by `emergency_switch_env` (defaults
  to `TB_STORAGE_FALLBACK_EMERGENCY`) is set to a truthy value (`1`, `true`,
  `yes`, `on`). This gives governance and SRE teams an emergency break-glass
  path without editing configuration files on every node.
- If the emergency switch is set, the node logs
  `storage_erasure_fallback_emergency` /
  `storage_compression_fallback_emergency` warnings so operators can audit when
  a rollout was driven by the override instead of the static config.

The CLI exposes the active algorithms via `tb storage manifests`, which prints
each manifest hash alongside the erasure/compression choices and highlights
fallback usage. The explorer mirrors this at
`/storage/manifests?limit=<N>`, returning a JSON document containing both the
per-manifest metadata and the current policy (active algorithm, whether the
fallback is permitted, and whether it is riding on the emergency switch).

Telemetry now tags latency and failure metrics with the coder and compressor
labels so dashboards can pivot on the rollout state:

- `storage_put_object_seconds{erasure="reed-solomon",compression="zstd"}`
  captures the end-to-end ingest latency per algorithm pair.
- `storage_put_chunk_seconds{…}` mirrors this for individual chunk dispatches.
- `storage_repair_failures_total{error="reconstruct",erasure="xor",compression="rle"}`
  shows which manifests failed during repair when fallback components are in
  use.

To quantify performance differences before flipping the switch, run the new
benchmark harness:

```bash
cargo run -p bench-harness -- compare-coders --bytes 1048576 --data 16 --parity 1 --iterations 64
```

The command prints average encode/decode latencies and throughput for the
default Reed–Solomon/Zstd stack versus the XOR/RLE fallback so incident teams
can make informed trade-offs during dependency incidents.

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

## Storage Engine Migration

The storage pipeline now honours the workspace-wide storage-engine
abstraction, allowing RocksDB, sled, or the in-memory engine to back the
provider registry and rent-escrow tables. When migrating an existing node:

1. **Back up the current data directory.** Stop the node and take an archive
   of the storage pipeline directories (typically `blobstore/`,
   `blobstore/compute_settlement.db`, and `blobstore/rent_escrow.db`).
2. **Run the migration tool.** `cargo run -p storage_migrate -- <old_dir>
   <new_dir> <engine>` rewrites the sled/RocksDB directories into the desired
   backend and verifies checksums. For example, convert a sled store to RocksDB
   with `cargo run -p storage_migrate -- blobstore blobstore.rocksdb rocksdb`.
3. **Update the node configuration.** Edit `config/default.toml` and set
   `storage.default_engine = "rocksdb"` or add overrides for
   `storage.overrides.storage_pipeline`/`storage.overrides.storage_fs` as
   required. Operators can temporarily set `storage_legacy_mode = true` to keep
   the previous behaviour for one release, but the CLI prints a deprecation
   warning and the toggle will be removed next cycle.
4. **Verify after restart.** `the-block storage providers` reports the active
   pipeline and rent-escrow engines and emits a warning when they diverge from
   the recommended default. Telemetry exposes
   `storage_engine_info{name="storage_pipeline",engine="rocksdb"}` so
   dashboards confirm the migration. Set `storage_legacy_mode` back to `false`
   once the new backend is stable.

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
