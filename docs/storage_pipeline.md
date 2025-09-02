# Storage Pipeline

The storage client splits objects into encrypted chunks before handing them to
providers. To keep uploads responsive across varied links, the pipeline adjusts
its chunk size on a per-provider basis:

For attack surfaces and mitigations see [threat_model/storage.md](threat_model/storage.md).

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
parity shards using Reed–Solomon coding (`1+1` configuration).  Shards are
round-robined across the provided storage backends, allowing any single shard to
be lost without data loss.  A manifest records the mapping of shard IDs to
provider IDs and includes a redundancy hint:

```json
{"version":1,"chunk_len":1048576,"redundancy":{"ReedSolomon":{"data":1,"parity":1}},...}
```

On retrieval the pipeline loads the manifest, fetches available shards, and
reconstructs missing data via `reed_solomon_erasure`.  Integration tests cover
shard loss and recovery under `node/tests/storage_erasure.rs`.

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
gateway appends a `ReadReceipt` `{domain, provider_id, bytes_served, ts}` under
`receipts/read/<epoch>/<seq>.cbor`. Hourly jobs Merklize these receipts and
write `receipts/read/<epoch>.root`; a settlement watcher moves the root to
`<epoch>.final` once the L1 anchor confirms and triggers `issue_read` to mint
subsidies from the global reward pool. Abuse is mitigated via in-memory
token buckets; exhausted buckets increment `read_denied_total{reason}` and still
append a `ReadReceipt` with `allowed=false` for audit trails. Dynamic pages emit
a companion `ExecutionReceipt` capturing CPU and disk I/O for the reward pool
while keeping reads free for users.

Rate limits throttle abusive bandwidth patterns without ever introducing
per-read fees, preserving the free-read guarantee for both owners and visitors.
