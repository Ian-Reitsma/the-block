# Compute Market Workloads

The compute market supports running real workloads on job slices. Each slice
contains input data that is executed by a workload runner and produces a proof
hash.

## Workload Formats

- **Transcode** – Accepts raw bytes representing media to be transcoded. For the
  reference implementation the bytes are hashed with BLAKE3 and the hash is
  returned as the slice proof.
- **Inference** – Accepts serialized model input bytes. The reference runner
  hashes the input with BLAKE3 and returns the hash as the proof.
- **GPUHash** – Offloads BLAKE3 hashing to a CUDA/OpenCL kernel when available
  and falls back to the CPU otherwise.

Both formats are deterministic; identical inputs always yield the same hash. The
`WorkloadRunner` dispatches to the appropriate reference job based on the
`Workload` enum and returns the proof hash for inclusion in `SliceProof`. Each
slice is processed in a `tokio::task::spawn_blocking` worker, allowing multiple
slices to execute in parallel. Results are cached per slice ID so repeated
invocations avoid recomputation. Parallel execution is deterministic—the same
inputs always yield the same hashes regardless of concurrency.

## Bid Commit–Reveal
Industrial bids use a post-quantum blind signature to hide parameters until reveal:

```
h = blind_sign_cat(salt, state)
```

`h` is broadcast in `BidTx`; after at least two blocks the bidder reveals `(salt,state)` and the signature is verified before execution.

Jobs may set `gpu_required = true` to schedule only on GPU-capable nodes; other
jobs run on any provider with deterministic CPU and GPU results checked by
tests.

## Slice Files and Hashing

Slices are raw byte blobs saved with a `.slice` extension. The reference
implementation simply hashes the contents with BLAKE3. Sample slice files and a
`generate_slice.py` helper live under `examples/workloads/`.

To run a sample workload:

```bash
cargo run --example run_workload examples/workloads/transcode.slice
```

## Courier Mode

Nodes can operate in a carry-to-earn mode by storing bundle receipts and
forwarding them when connectivity is restored. Use the CLI to manage receipts:

```bash
# store a receipt for bundle.bin from sender alice
node compute courier send bundle.bin alice
# forward all pending receipts
node compute courier flush
```

Receipts are persisted in `sled` until acknowledged and rewards are paid
when forwarding succeeds. Each receipt carries a unique ID and an
`acknowledged` flag set only after successful forwarding. The `compute courier
flush` command retries failed sends with exponential backoff and records
`courier_flush_attempt_total` and `courier_flush_failure_total` Prometheus
counters for observability. Flushing streams entries directly from the
underlying database iterator, so memory usage remains constant even with large
receipt queues.

Integration tests exercise the backoff logic and metric counters. Run

```bash
cargo nextest run --features telemetry compute_market::courier_retry_updates_metrics
```

to verify the retry behaviour and metrics.

For a deep dive into receipt fields, storage paths, and retry semantics see [docs/compute_market_courier.md](compute_market_courier.md).

## Price Board Persistence

Recent offer prices feed a sliding window that derives quantile bands. The board
persists to `node-data/state/price_board.v1.bin` on shutdown and every
`price_board_save_interval` seconds. If the file is missing or corrupted the
board starts empty. The persistence path, window size, and save interval are
configurable via `node-data/config.toml`:

```toml
price_board_path = "state/price_board.v1.bin"
price_board_window = 100
price_board_save_interval = 30
```

Each entry is an unsigned 64‑bit integer, so disk usage is `8 * price_board_window`
bytes (≈800 B for the default). Older prices are dropped as new ones arrive.

Clear the board by deleting the persistence file:

```bash
rm node-data/state/price_board.v1.bin
```

Logs emit `loaded price board` and `saved price board` messages, while metrics
`price_band_p25{lane}`, `price_band_median{lane}`, and `price_band_p75{lane}`
track quantile bands. Suggested bids are adjusted by a backlog factor of
`1 + backlog/window` computed per lane, excluding deferred industrial jobs.

## Receipt Settlement

Matches between bids and asks produce `Receipt` objects that debit CT from the
buyer and pay the provider. Settlement tracks applied receipts in a sled tree to
guarantee idempotency across restarts.

```text
{ version: 1, job_id, buyer, provider, quote_price, issued_at, idempotency_key }
```

The `idempotency_key` is `BLAKE3(job_id || buyer || provider || quote_price ||
version)` and is used as the primary index in the `ReceiptStore`.  Receipts are
persisted via `compare_and_swap` so duplicates are ignored.  On startup the
store reloads existing entries and increments `receipt_corrupt_total` for any
damaged records.

`Settlement` operates in one of three modes:

- **DryRun** – records receipts without moving balances (default).
- **Armed** – after a configured delay, transitions to `Real`.
- **Real** – debits buyers and pays providers in CT.

Operators can toggle modes via CLI and inspect the current state with the
`settlement_status` RPC, which reports balances and mode.

When `Real`, each finalized receipt subtracts `quote_price` from the buyer and
accrues the same amount for the provider with an event tag.  Failures (e.g.,
insufficient funds) are archived and cause the system to revert to `DryRun`.
Metrics track behaviour:

- `settle_applied_total` – receipts successfully debited and paid.
- `settle_failed_total{reason}` – settlement failures.
- `settle_mode_change_total{to}` – mode transitions.
- `matches_total{dry_run}` – receipts processed by the match loop.
- `receipt_persist_fail_total` – database write errors.
- `match_loop_latency_seconds` – time per match-loop iteration.

The `settlement_cluster` test exercises idempotency by applying receipts across
node restarts and asserting that metrics record exactly one application.

## Industrial Admission & Fee Lanes

Every transaction declares a `lane` identifying it as `Consumer` or
`Industrial`. Transactions queue in separate mempools per lane so consumer
traffic stays latency sensitive. Industrial jobs are admitted only when a
moving-window capacity estimator reports enough shard headroom and the comfort
guard is `loose`. When median consumer fees drift above the configured
`comfort_threshold_p90` or the consumer mempool's age p95 exceeds limits, the
system enters a `tight` mode that rejects new industrial jobs until conditions
recover.

Telemetry gauges and counters surface admission behaviour:

- `mempool_size{lane}`
- `consumer_fee_p50`, `consumer_fee_p90`
- `industrial_admitted_total`
- `industrial_deferred_total`
- `industrial_rejected_total{reason}`
- `admission_mode{mode}`
- `industrial_rejected_total{reason="SLA"}` – slashes for missed provider deadlines

These metrics drive Grafana panels tracking fee health and industrial
throttling. Future patches will expose user-facing rejection codes and make
the comfort threshold governable.

Admission decisions are logged with job identifiers, requested shards, and current mode to aid post-mortems.

Providers that miss declared job deadlines have their bonds slashed via `penalize_sla`, incrementing `industrial_rejected_total{reason="SLA"}` for dashboard alerts. Operators should set Prometheus rules to page when this counter rises.

## Fair-Share Caps and Burst Quotas

Admission uses a dual budget model to prevent any single buyer or provider from
monopolizing industrial capacity.  Each call to
`check_and_record(buyer, provider, demand)` in
[`admission.rs`](../node/src/compute_market/admission.rs) evaluates two limits:

1. **Fair-share cap** – The moving window of demand for each party is compared
   against total observed capacity. The cap defaults to
   `FAIR_SHARE_CAP_MICRO = 250_000`, i.e. 25% of the window.
2. **Burst quota** – When the fair-share cap is exceeded, a short-term bucket
   allows extra throughput. `BURST_QUOTA` defaults to 30 micro-shard-seconds and
   refills at `BURST_REFILL_RATE_MICRO` (0.5 µshard·s per second) until the cap
   is replenished.

Usage decays linearly over a 60 s window.  For each buyer and provider a
`Usage` record stores `shards_seconds`; the value is multiplied by
`(WINDOW_SECS - elapsed)/WINDOW_SECS` whenever checked. Quotas are tracked in
`Quota` structs and refilled by the same decay timer.  Both limits are evaluated
before jobs enter the market, returning `RejectReason::{Capacity,FairShare,BurstExhausted}`
on failure.

### Querying Admission Parameters

Governance proposals of type `Admission` can retune the budgets at runtime.
Parameter keys map to fields in `governance::params::Params` and apply via
`Runtime::set_min_capacity`, `set_fair_share_cap`, and `set_burst_refill_rate`.
Operators can inspect current values through the CLI:

```bash
blockctl compute-market params
```

Example JSON output:

```json
{
  "min_capacity": 10,
  "fair_share_cap": 0.25,
  "burst_quota": 30.0,
  "burst_refill_rate": 0.5
}
```

### Troubleshooting

- `INDUSTRIAL_REJECTED_TOTAL{reason="fair_share"}` – buyer or provider exceeded
  the global cap. Verify quotas with `compute-market params` and check the
  `fair_share_cap` value.
- `INDUSTRIAL_REJECTED_TOTAL{reason="burst_exhausted"}` – burst bucket is empty.
  Wait for refill or raise `burst_refill_rate` via governance.
- `INDUSTRIAL_REJECTED_TOTAL{reason="capacity"}` –
  `record_available_shards` reports insufficient headroom. Ensure providers are
  advertising realistic capacity.

Simulation scenarios exercising admission behaviour live under
[`sim/compute_market`](../sim/compute_market/).

## Developer notes

When modifying compute-market code, run `cargo nextest run --features telemetry compute_market::courier_retry_updates_metrics price_board` in addition to the full test suite to exercise persistence paths.
