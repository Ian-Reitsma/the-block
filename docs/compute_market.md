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

Both formats are deterministic; identical inputs always yield the same hash. The
`WorkloadRunner` dispatches to the appropriate reference job based on the
`Workload` enum and returns the proof hash for inclusion in `SliceProof`. Each
slice is processed in a `tokio::task::spawn_blocking` worker, allowing multiple
slices to execute in parallel. Results are cached per slice ID so repeated
invocations avoid recomputation. Parallel execution is deterministic—the same
inputs always yield the same hashes regardless of concurrency.

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
persists to `node-data/price_board.bin` on shutdown and reloads on startup. If
the file is missing or corrupted the board starts empty. The persistence path
and window size are configurable via `node-data/config.toml`:

```toml
price_board_path = "price_board.bin"
price_board_window = 100
```

Each entry is an unsigned 64‑bit integer, so disk usage is `8 * price_board_window`
bytes (≈800 B for the default). Older prices are dropped as new ones arrive.

Clear the board by deleting the persistence file:

```bash
rm node-data/price_board.bin
```

Logs emit `loaded price board` and `saved price board` messages, while metrics
`price_band_p25`, `price_band_median`, and `price_band_p75` track quantile
bands.

## Dry-Run Settlement Loop

The compute market currently runs in a dry-run mode that forms notional matches
between bids and asks without transferring real fees. Each successful match
emits a `Receipt`:

```text
{ version: 1, job_id, buyer, provider, quote_price, dry_run: true,
  issued_at, idempotency_key }
```

The `idempotency_key` is `BLAKE3(job_id || buyer || provider || quote_price ||
version)` and is used as the primary index in the `ReceiptStore`. Receipts are
persisted via `sled` with `compare_and_swap` to guarantee exactly-once
insertion. On startup the store reloads existing entries and skips corrupted
records, incrementing the `receipt_corrupt_total` counter.

`match_loop` greedily pairs bids and asks on a fixed interval and records
metrics:

- `matches_total{dry_run}` – receipts successfully persisted.
- `receipt_persist_fail_total` – database write errors.
- `match_loop_latency_seconds` – time spent per loop iteration.

Dry-run mode is the default and can be disabled with the `--dry-run=false`
flag on `node run`. Once the receipt path is battle-tested the same interface
can attach real debits and credits by toggling this flag.

## Industrial Admission & Fee Lanes

Every transaction declares a `lane` identifying it as `Consumer` or
`Industrial`. Consumer traffic always takes priority. Industrial jobs are
admitted only when a moving-window capacity estimator reports enough shard
headroom. If median consumer fees drift above the configured
`comfort_threshold_p90`, the system enters a `tight` admission mode that
defers new industrial jobs until fees recover.

Telemetry gauges and counters surface admission behaviour:

- `consumer_fee_p50`, `consumer_fee_p90`
- `industrial_admitted_total`
- `industrial_deferred_total`
- `admission_mode{mode}`

These metrics drive Grafana panels tracking fee health and industrial
throttling. Future patches will expose user-facing rejection codes and make
the comfort threshold governable.

Admission decisions are logged with job identifiers, requested shards, and current mode to aid post-mortems.

## Developer notes

When modifying compute-market code, run `cargo nextest run --features telemetry compute_market::courier_retry_updates_metrics price_board` in addition to the full test suite to exercise persistence paths.
