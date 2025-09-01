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

Receipts are persisted in `sled` until acknowledged and rewards are credited
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

## Receipt Settlement and Credits Ledger

Matches between bids and asks produce `Receipt` objects that debit the buyer and
credit the provider. Settlement uses the [`credits` ledger](credits.md) crate and
tracks applied receipts in a sled tree to guarantee idempotency across restarts.

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
- **Real** – debits buyers and accrues credits to providers.

Operators can toggle modes via CLI and inspect the current state with the
`settlement_status` RPC, which reports balances and mode.

When `Real`, each finalized receipt subtracts `quote_price` from the buyer and
accrues the same amount for the provider with an event tag.  Failures (e.g.,
insufficient funds) are archived and cause the system to revert to `DryRun`.
Metrics track behaviour:

- `settle_applied_total` – receipts successfully debited and credited.
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

## Developer notes

When modifying compute-market code, run `cargo nextest run --features telemetry compute_market::courier_retry_updates_metrics price_board` in addition to the full test suite to exercise persistence paths.
