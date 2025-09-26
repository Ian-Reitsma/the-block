# Storage Pipeline
> **Review (2025-09-25):** Synced Storage Pipeline guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The storage client splits objects into encrypted chunks before handing them to
providers. Completed blob roots are queued for on-chain anchoring by the
[BlobScheduler](blob_chain.md), which separates light L2 roots from heavy L3
roots and releases them on 4 s and 16 s cadences respectively. To keep uploads
responsive across varied links, the pipeline adjusts its chunk size on a
per-provider basis:

For attack surfaces and mitigations see [threat_model/storage.md](threat_model/storage.md).

Beginning with the multi-provider coordinator, the pipeline derives a logical
quota for each provider from `Settlement::balance_split(provider)` (1 credit →
1 MiB). Providers exceeding their quota, failing recent uploads, or explicitly
flagged for maintenance are skipped when selecting chunk targets. The manifest
now records `chunk_lens` and `provider_chunks` so heterogeneous chunk sizes can
be reconstructed during re-downloads; each `provider_chunks` entry stores the
chunk indices, plain lengths, and derived per-provider encryption keys.

- Allowed sizes: 256 KiB, 512 KiB, 1 MiB, 2 MiB, 4 MiB
- Target chunk time: ~3 s
- Per-chunk throughput, RTT, and loss are folded into an EWMA profile stored in
  `provider_profiles/{node_id}`.
- The preferred chunk size only changes after at least three stable chunks and
  shifts by one ladder step at a time. High loss (>2 %) or RTT (>200 ms) forces
  a downgrade; exceptionally clean links (<0.2 % loss, RTT <80 ms) allow
  upgrades.

Metrics exported via the telemetry feature include:

- `storage_chunk_size_bytes`
- `storage_put_chunk_seconds`
- `storage_provider_rtt_ms`
- `storage_provider_loss_rate`
- `storage_initial_chunk_size`
- `storage_final_chunk_size`
- `storage_put_eta_seconds`

Profiles persist across restarts so subsequent uploads reuse the last known
chunk size. The `profile_persists_across_multiple_restarts` test restarts a node
twice and asserts that the provider profile and chosen chunk size remain
constant.

## Erasure Coding and Multi-Provider Placement

Each chunk is encrypted with ChaCha20-Poly1305 and then split into data and
parity shards via the shared `coding` crate. Defaults come from
[`config/storage.toml`](../config/storage.toml) and currently request 16 data
shards and 8 parity shards backed by Reed–Solomon
(`crates/coding/src/erasure.rs`). A manifest records the mapping of shard IDs to
provider IDs and stores the active algorithm and counts so repair logic can
select the matching coder:

```json
{"version":1,"chunk_len":1048576,"erasure_alg":"reed-solomon","erasure_data":16,"erasure_parity":8,"compression_alg":"zstd",...}
```

Operators may opt into the in-house XOR fallback by setting
`erasure.algorithm = "xor"` and enabling the rollout gate in
`[rollout]` within `config/storage.toml`. The fallback produces a single parity
vector (duplicated to satisfy manifest layout), trades redundancy for supply-chain
independence, and causes the repair loop to log
`algorithm_limited:<alg>:missing=<n>:parity=<m>` when reconstruction would be
impossible. Integration coverage exercises both paths in
`storage/tests/fallback_coder.rs` and `storage/tests/repair.rs`.

Compression choices follow the same configuration path (`compression.algorithm`
and `rollout.allow_fallback_compressor`) and manifest field so retrieval knows
whether to invoke zstd or the lightweight RLE fallback.

Shards are round-robined across storage backends so the pipeline tolerates up to
the configured parity shard loss per chunk. Algorithm choices load through
`node/src/storage/settings.rs`, and telemetry tags
`storage_put_object_seconds`, `storage_put_chunk_seconds`, and
`storage_repair_failures_total` with `erasure`/`compression` labels to make
rollout dashboards trivial. The bench harness offers a
`compare-coders` command that benchmarks Reed–Solomon+zstd versus XOR+RLE
(`tools/bench-harness/src/main.rs`).

## Provider Catalog Health Checks

Uploads consult a `NodeCatalog` that tracks registered storage providers. Each
provider exposes a `probe()` method returning an estimated RTT or an error. The
catalog periodically probes all entries, prunes those reporting timeouts or
excessive loss, and ranks the remainder by recent latency. During `put_object`
the pipeline selects the healthiest providers from this catalog. See
[`node/tests/provider_catalog.rs`](../node/tests/provider_catalog.rs) for
examples.

## Background Repair Loop

`node/src/storage/repair.rs` spawns a periodic task that scans manifests and
reconstructs missing shards. Rebuilt bytes are written back to the local store
and counted via `storage_repair_bytes_total`; failures increment
`storage_repair_failures_total`. The asynchronous job runs every few seconds and
keeps redundancy intact even if a chunk is lost. For a demonstration, consult
[`node/tests/storage_repair.rs`](../node/tests/storage_repair.rs).

## Free Reads and Receipts

Gateway fetches are free for clients and domain owners. After serving bytes the
gateway appends a `ReadAck` `{manifest, path_hash, bytes, ts, client_hash, pk,
sig}` under `receipts/read/<epoch>/<seq>.cbor`. Hourly jobs Merklize these
acknowledgements and write `receipts/read/<epoch>.root`; a settlement watcher
moves the root to `<epoch>.final` once the L1 anchor confirms and triggers
`issue_read` to mint subsidies from the global reward pool. Abuse is mitigated
via in-memory token buckets; exhausted buckets increment
`read_denied_total{reason}` and still append a `ReadAck` with `allowed=false`
for audit trails. Dynamic pages emit a companion `ExecutionReceipt` capturing
CPU and disk I/O for the reward pool while keeping reads free for users.

See [docs/read_receipts.md](read_receipts.md) for the full acknowledgement
format, batching algorithm, and audit tooling.

Rate limits throttle abusive bandwidth patterns without ever introducing
per-read fees, preserving the free-read guarantee for both owners and visitors.
