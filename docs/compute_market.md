# Compute Market Workloads
> **Review (2025-09-25):** Synced Compute Market Workloads guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The compute market supports running real workloads on job slices. Each slice
contains input data that is executed by a workload runner and produces a proof
hash.

## CT Escrow

Offers specify `fee_pct_ct`, the consumer-token percentage of the quoted price.
Policy pins production lanes to `100`, so live jobs settle entirely in CT while
leaving the selector available for simulations and regression tests. During
admission, buyers escrow the computed CT amount and settlement credits providers
with the same figure. Example selector values (used in tests/devnets) remain:

| `fee_pct_ct` | CT share | Legacy industrial share |
|--------------|---------|-------------------------|
| `0`          | `0%`    | `100%` (tests only)     |
| `25`         | `25%`   | `75%` (tests only)      |
| `100`        | `100%`  | `0%`                    |

Residual escrows are refunded using the submitted selector; production payouts
therefore return solely CT.

## Settlement Ledger & Auditing

Settlement persists CT flows (with a legacy industrial field fixed to zero in production) in a first-party ledger located under `compute_settlement.db`. `Settlement::init` wires the compatibility handle (`storage-rocksdb`) to the in-house engine and
transparently falls back to an in-memory ledger for tests. Every accrual writes dual entries, updates a
rolling Merkle root cache, records activation metadata, and bumps a monotonic
sequence so operators can replay state after restarts. The ledger exposes:

- **CLI:** Build `contract-cli` with `--features full` and invoke `contract compute stats` to report
  CT balances alongside the most recent audit entries fetched via
  `compute_market.provider_balances` and `compute_market.audit`.
- **RPC:**
  - `compute_market.provider_balances` returns the merged CT totals for every
    provider persisted in the ledger, sorted lexicographically to match the
    Merkle-root fold (`compute_root`) and encoded as `{provider, ct, industrial}`
    structs. The `industrial` field remains for compatibility and is zeroed on
    production lanes.
  - `compute_market.audit` streams JSON receipts suitable for automated
    reconciliation. Each entry mirrors `AuditRecord` in
    `node/src/compute_market/settlement.rs` and includes the CT deltas,
    running balances, timestamp, sequence number, and optional anchor hash.
  - `compute_market.recent_roots` exposes the last 32 Merkle roots (or a caller
    supplied limit) as hex strings so explorers can render continuity proofs
    directly from the ledger.
- **Explorer:** `explorer/src/compute_view.rs` renders provider balances,
  anchors, and audit logs directly from the persisted ledger, relying on the
  RPCs above.

Integration tests in `node/tests/compute_settlement.rs` verify persistence,
refund flows, anchoring, and activation mode transitions. Telemetry counters
`COMPUTE_SLA_VIOLATIONS_TOTAL`, `SLASHING_BURN_CT_TOTAL`, `SETTLE_APPLIED_TOTAL`,
`SETTLE_FAILED_TOTAL{reason}`, and `SETTLE_MODE_CHANGE_TOTAL{state}` increment on
each action, feeding dashboards with SLA burn visibility and mode tracking. The
log file written via `state::append_audit` stores anchors alongside the JSON
audit feed for offline reconciliation.

### Activation Modes & Metadata

`SettleMode` tracks whether receipts immediately settle on-chain:

| Mode | Description | Persistence metadata |
| --- | --- | --- |
| `DryRun` | Default safety mode. Balances accrue in the ledger without moving CT on-chain. Use for devnets or smoke tests. | `metadata.armed_requested_height` and `metadata.armed_delay` are cleared; `last_cancel_reason` documents why the node fell back. |
| `Armed { activate_at }` | Governance has requested activation after `delay` blocks. Ledger persists the requested height and delay so a restart cannot skip the waiting period. | Fields capture the requested height and delay until activation or cancellation. |
| `Real` | Full settlement—balances are authoritative and should be anchored into `state::audit`. | Anchors append to `metadata.last_anchor_hex` and the audit log. |

Operators can request activation through the RPC surface (or configuration) by
calling `Settlement::arm(delay, current_height)` and revert with
`Settlement::cancel_arm()` or `Settlement::back_to_dry_run(reason)`. All paths
persist metadata immediately and flush RocksDB on shutdown so a crash cannot
skip the arming delay.

### Anchoring and Refunds

- `Settlement::submit_anchor` hashes submitted receipts, appends a durable JSON
  line via `state::append_audit`, and records a marker audit entry with the
  anchor hash.
- `Settlement::refund_split` and `Settlement::accrue_split` always update both
  ledgers atomically. The recorded audit entry shows the CT delta alongside the legacy industrial column (zeroed in production) with the updated balances.
- `Settlement::penalize_sla` burns CT from the provider, records the event with
  a negative delta, and increments `SLASHING_BURN_CT_TOTAL` plus
  `COMPUTE_SLA_VIOLATIONS_TOTAL{provider}` to highlight SLA breaches.

### SLA Automation & Dashboards

Settlement now maintains an explicit SLA queue so overdue jobs are swept
automatically without relying on manual operator intervention. Every call to
`Settlement::track_sla` persists the provider/consumer bonds, deadline, and the
submission timestamp. A background sweep (`Settlement::sweep_overdue`) or any
job lifecycle transition inside `Market` enforces deadlines by burning the
provider bond, refunding the consumer bond, and appending a structured
`SlaResolution` record for dashboards to consume.

Telemetry exposes the live queue via:

- `COMPUTE_SLA_PENDING_TOTAL` – gauge of queued SLA records.
- `COMPUTE_SLA_NEXT_DEADLINE_TS` – unix timestamp of the next deadline so
  Grafana panels can show time-to-breach.
- `COMPUTE_SLA_AUTOMATED_SLASH_TOTAL` – counter of automated slashes triggered
  by the sweep.

The settlement JSON audit log records the last violation in
`metadata.last_sla_violation`, and CLI/RPC surfaces surface the same state so
operators can correlate burned CT with job identifiers. Dashboards should plot
the pending gauge alongside `SLASHING_BURN_CT_TOTAL` to ensure the automatic
enforcement path remains healthy.

Sample RPC calls (adjust the node URL as needed):

```bash
curl -s localhost:26658/compute_market.provider_balances | jq
curl -s localhost:26658/compute_market.audit | jq '.[0]'
curl -s localhost:26658/compute_market.recent_roots | jq '.roots'
```

Each audit object resembles:

```json
{
  "sequence": 19,
  "timestamp": 1695206400,
  "entity": "provider-nyc-01",
  "memo": "accrue_split",
  "delta_ct": 4200,
  "delta_it": 600,
  "balance_ct": 98200,
  "balance_it": 14000,
  "anchor": null
}
```

Use the anchor entries (`entity == "__anchor__"`) to correlate local ledger state with explorer or archival tooling. The `*_it` fields remain for compatibility and surface non-zero values only in explicit test scenarios.

## Lane-Aware Matching & Fairness

The matcher maintains independent order books per `FeeLane` so consumer and
industrial flows never starve one another. Each lane keeps sorted bid/ask queues
ordered by price-time priority and honours a configurable fairness window from
`LaneMetadata`. Bids that arrive within the fairness window are rotated in FIFO
order so newer, higher-priced jobs cannot eclipse older entries indefinitely.
`stable_match` processes one lane at a time, ensuring lane tags on receipts
match before settlement persists them.

Lane metadata travels with every seed. `seed_orders` validates that each lane
stays within its configured `max_queue_depth`; exceeding the cap returns a
`CapacityExceeded` error rather than evicting live orders. Operators can stage
seeds in memory, validate capacity, and then atomically replace the live book so
invalid staging data never wipes active queues.

`LaneStatus` exposes bid/ask depth alongside `oldest_bid_wait`/`oldest_ask_wait`
durations, while `LaneWarning` records the job ID, age, and wall-clock timestamp
for starvation reports. Both structs surface through `compute_market.stats` and
the CLI to help operators correlate fairness windows with live queue health.

### Batch Controls & Back-Pressure

Background matching processes work in configurable batches. Set
`TB_COMPUTE_MATCH_BATCH` to bound the number of receipts `match_loop` commits per
tick; when a batch fills completely the loop yields to give other async tasks
time on the runtime. Between batches the loop sleeps for `MATCH_INTERVAL` (250
ms) so sustained bursts do not starve other subsystems.

Each lane enforces back-pressure through its `max_queue_depth`. When a lane is
full the matcher rejects further orders and emits a warning via `tracing`. This
prevents runaway memory use if a client floods a lane faster than providers can
clear it. CLI and RPC surfaces expose current queue depth so operators can
expand capacity or rebalance senders before the queue saturates.

### Starvation Detection & Operator Signals

`refresh_starvation` walks every lane after each batch to detect bids that have
waited longer than the configured fairness window. When a lane crosses the
threshold the node logs a `lane starvation` warning with the offending job ID.
`compute_market.stats` reports a `lane_starvation` array containing structured
warnings (`lane`, `job_id`, `waited_for_secs`, `updated_at`). The CLI prints the
same context when you run `contract compute stats`, making it obvious when a
lane is stuck behind missing providers or policy misconfiguration.

### Telemetry & Observability

Prometheus metrics now include:

- `matches_total{dry_run="false",lane="consumer"}` – successful matches per lane
  split by dry-run state.
- `match_loop_latency_seconds{lane}` – histogram of per-lane batch latency so
  dashboards can highlight congestion.
- `receipt_persist_fail_total` – persistence failures while committing lane-tagged
  receipts.

Cluster dashboards should pair these counters with the existing settlement
metrics to ensure receipts flow smoothly.

### CLI & RPC Lane Views

`compute_market.stats` returns per-lane queue depth and starvation hints in the
`lanes` and `lane_starvation` fields. The CLI surfaces this with
`contract compute stats --url http://localhost:26658`, printing bid/ask counts
and queue ages per lane. For raw JSON, call the RPC directly:

```bash
curl -s localhost:26658/compute_market.stats | jq '.lanes, .lane_starvation'
```

A typical response resembles:

```json
{
  "lanes": [
    {
      "lane": "Consumer",
      "bids": 6,
      "asks": 4,
      "oldest_bid_wait_secs": 12.4,
      "oldest_ask_wait_secs": 2.1
    }
  ],
  "lane_starvation": [
    {
      "lane": "Consumer",
      "job_id": "job-nyc-91",
      "waited_for_secs": 32.8,
      "updated_at": "2025-09-25T10:02:17Z"
    }
  ]
}
```

The CLI mirrors these fields, highlighting lanes whose oldest bid exceeded the
fairness window and including the timestamp of the most recent warning so teams
can align dashboards with manual interventions.

## Normalized Compute Units

Workloads are expressed in **compute units** representing GPU-seconds scaled by
device throughput. The reference implementation estimates units as one per MiB
of workload input. Providers post offers with a `units` capacity and
`price_per_unit`, and receipts include the units consumed. Prometheus gauges
`industrial_units_total` and `industrial_price_per_unit` track aggregate demand
and the latest quoted price. Hardware can be calibrated via
`compute_market::workload::calibrate_gpu`, which maps a GPU's GFLOPS rating to
units per second.

### Demand Metrics and Subsidy Interaction

The marketplace exposes `industrial_backlog` and `industrial_utilization`
gauges. `industrial_backlog` counts queued compute units awaiting execution,
while `industrial_utilization` reports realised throughput over the current
window as an integer percentage. The subsidy governor samples these metrics through
`Block::industrial_subsidies()` when retuning multipliers, tying pricing to
actual demand.

Sample stats output:

```bash
curl localhost:26658/compute_market.stats
```

```json
{
  "industrial_backlog": 12,
  "industrial_utilization": 83,
  "industrial_units_total": 240,
  "industrial_price_per_unit": 5,
  "industrial_price_weighted": 6,
  "industrial_price_base": 4,
  "pending": [],
  "lanes": [
    {
      "lane": "consumer",
      "bids": 3,
      "asks": 2,
      "oldest_bid_wait_ms": 1800,
      "oldest_ask_wait_ms": 0
    },
    {
      "lane": "industrial",
      "bids": 1,
      "asks": 4,
      "oldest_bid_wait_ms": null,
      "oldest_ask_wait_ms": 4500
    }
  ],
  "lane_starvation": []
}
```

High backlog with low utilisation suggests providers are scarce; governance may
raise multipliers or admission targets. Cross-reference
[docs/inflation.md](inflation.md) for how these gauges feed the retuning
mechanics.

## SNARK Receipts

Workloads may include Groth16/Plonk proofs for their output hashes. The
`ExecutionReceipt` carries an optional `proof` field; when present the scheduler
verifies it with `compute_market::snark::verify` before crediting payment.
Successful checks increment `snark_verifications_total` and failures bump
`snark_fail_total`. Receipts without proofs follow the legacy trust-based path
and remain compatible with existing workloads.

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
`Workload` enum and returns the proof hash for inclusion in an `ExecutionReceipt`. Each
slice is processed via the shared runtime's `spawn_blocking` helper, allowing multiple
slices to execute in parallel. Results are cached per slice ID so repeated
invocations avoid recomputation. Parallel execution is deterministic—the same
inputs always yield the same hashes regardless of concurrency.

## Bid Commit–Reveal
Industrial bids use a post-quantum blind signature to hide parameters until reveal:

```
h = blind_sign_cat(salt, state)
```

`h` is broadcast in `BidTx`; after at least two blocks the bidder reveals `(salt,state)` and the signature is verified before execution.

### Capability-Based Scheduling

Providers attach a capability descriptor and a `reputation_multiplier` to each
offer. Jobs specify the minimum capability they require. The scheduler computes
an **effective price** as `price_per_unit * reputation_multiplier` and, when an
accelerator is requested, multiplies by a fixed `1.2×` accelerator premium. It
selects the matching provider with the lowest effective price among those with
non-negative reputation. The multiplier range is bounded by
`reputation_multiplier_min`/`reputation_multiplier_max` in `config.toml` and
defaults to `0.5–1.0`. Reputation starts from the offer's advertised score and is
incremented on successful completion or decremented on failure.

### Scheduler Flow

![Scheduler flow](assets/scheduler_flow.svg)

The scheduler ingests offers, applies reputation multipliers to derive effective
prices, and selects the lowest-cost provider that satisfies the job's capability
requirements.

Prometheus counters `scheduler_match_total{result}` and
`reputation_adjust_total{result}` expose scheduler outcomes and reputation
adjustments. The gauge `scheduler_effective_price{provider}` records the latest
effective price by provider. Unmatched accelerator requests increment
`scheduler_accelerator_miss_total`. Snapshot recent success, failure counts, and
the last effective price with the `compute_market.scheduler_stats` RPC or `net
compute stats --effective` CLI. Query job capability descriptors through
`compute.job_requirements`. Reputation scores persist across restarts in
`~/.the_block/reputation.json` and decay toward zero at the rate configured by
`provider_reputation_decay`.

### Fair-Share Scheduling

Queued jobs age over time and gain an *effective priority* boost bounded by
`max_priority_boost`. The aging rate is controlled by `aging_rate` and prevents
low-priority work from starving when high-priority traffic dominates. The
scheduler persists enqueue timestamps so restarts do not reset aging. Operators
can inspect the aged queue with `compute queue` and monitor
`job_age_seconds` and `priority_boost_total` metrics to tune fairness.

### Reputation Gossip

Nodes exchange provider scores using a lightweight gossip message carrying
`(provider_id, reputation_score, epoch)` tuples. A manual round can be
triggered with `net reputation sync`, which broadcasts the current snapshot to
known peers. Incoming entries replace local scores only if their epoch is
greater than the stored value and the score lies within `[-1000,1000]`. The
`reputation_gossip_total{result="applied|ignored"}` counter tracks update
processing alongside `reputation_gossip_latency_seconds` and
`reputation_gossip_fail_total`. Set `reputation_gossip = false` in `config.toml` to opt out of the
protocol.

### Preemption

When `enable_preempt` is set in `config.toml`, the scheduler may migrate an
active job to a higher-reputation provider. An incoming offer whose reputation
exceeds the running provider by at least `preempt_min_delta` triggers a handoff
via the courier. Successful migrations increment
`scheduler_preempt_total{reason="success"}`; handoff failures increment
`scheduler_preempt_total{reason="handoff_failed"}` and leave the original
assignment intact. Preemption counts are exposed through the
`compute_market.scheduler_stats` RPC.

Preemption decreases the displaced provider's reputation and logs the event in
`cancellations.log` with reason `preempted`. The CLI lists these using
`tb-cli compute list --preempted`. Resources on the old node are halted before
handoff to avoid double execution.

### Cancellations

Consumers or providers may abort an active job via `compute.job_cancel`. The
scheduler releases the slot, refunds posted bonds, and records the event in a
`cancellations.log` ledger for replay protection. The counter
`scheduler_cancel_total{reason="client|provider|preempted"}` tracks aggregate
reasons. Provider reputation adjusts based on the supplied reason: client-driven
cancels do not penalise the worker, while provider faults decrement their
score. Repeated provider-initiated cancellations may incur settlement
penalties.

Cancel a job from the CLI with:

```bash
tb-cli compute cancel <job_id>
```

Sample output on success:

```text
cancelled job abc123
```

Attempting to cancel a completed job returns:

```text
job already completed
```

### Reputation Weighting

Providers advertise a base price per compute unit. A reputation-derived weight
`w`, clamped within configured bounds, adjusts the price to reward reliable
participants:

```
effective_price = base_price * w
```

Weighted and raw median prices surface via `compute_market.stats` under
`industrial_price_weighted` and `industrial_price_base`, while the canonical
spot price is exposed through `industrial_price_per_unit`. Each adjusted entry
increments the `price_weight_applied_total` counter.

Exit code `0` indicates success; non-zero codes map to `not_found` or
`already_done` errors. Refunds surface in `compute_market_fee_split`
metrics and the wallet balance. Each cancellation is appended to
`cancellations.log` for replay protection, and reputation impacts are
defined in `node/src/compute_market/scheduler.rs`.

To cancel multiple jobs in bulk:

```bash
for id in $(cat jobs.txt); do tb-cli compute cancel "$id" || true; done
```

Troubleshooting:

- **Timeouts** – increase RPC timeout or verify network reachability.
- **Courier handoff errors** – ensure the courier has acknowledged the
  cancel; reissue if necessary.

See [docs/gossip.md](gossip.md) for general CLI conventions and transport
flags.

### Priority and Aging

Workloads may supply a `priority` field (`low`, `normal`, or `high`) when
submitting a job. The scheduler maintains a priority queue ordered first by
priority and then by arrival time. Older high-priority jobs run before newer
low-priority ones, ensuring fairness under load.

Operators can cap the share of concurrently executing low-priority jobs via
`low_priority_cap_pct` in `config.toml`. Jobs exceeding this cap wait in the
queue until capacity frees. The counter `scheduler_priority_miss_total`
increments if a non-low-priority job waits more than five seconds before
starting. Queue depths and miss counts are available through the
`compute_market.scheduler_stats` RPC.

## Slice Files and Hashing

Slices are raw byte blobs saved with a `.slice` extension. The reference
implementation simply hashes the contents with BLAKE3. Sample slice files and a
`generate_slice.py` helper live under `examples/workloads/`.

`gpu_inference.json` demonstrates requesting an RTX4090-capable provider with 16 GB of VRAM. Additional examples illustrate a
CPU-only task (`cpu_only.json`), a multi-GPU request (`multi_gpu.json`), and
accelerator-driven workloads (`tpu_inference.json`, `fpga_inference.json`):

```bash
cat examples/workloads/gpu_inference.json
```

To run a sample workload:

```bash
cargo run --example run_workload examples/workloads/transcode.slice
```

Fetch aggregated scheduler statistics with:

```bash
curl -s -d '{"method":"compute_market.scheduler_stats"}' http://localhost:3030 | jq
```

Sample output:

```json
{"success":1,"capability_mismatch":0,"reputation_failure":0,"active_jobs":0,"utilization":{"A100":1}}
```

### GPU Memory Matching

Providers advertise `gpu_memory_mb` alongside the GPU model. Workloads may
require a minimum amount of VRAM, and the scheduler will only match offers that
meet both the model and memory requirements. If no provider has sufficient
memory, the scheduler increments `scheduler_match_total{result="capability_mismatch"}`
for observability.

### Accelerator Requests

Jobs may request specialized accelerators such as TPUs or FPGAs. Providers
advertise an `accelerator` field taking `FPGA` or `TPU` plus
`accelerator_memory_mb` in their capability descriptor. When a job's
accelerator requirement cannot be met—either because no provider offers the
requested model or available memory is insufficient—the
`scheduler_accelerator_miss_total` counter increments. Successfully scheduled
accelerator jobs bump `scheduler_accelerator_util_total` while failures or
cancellations bump `scheduler_accelerator_fail_total`.

Default reputation decay and retention values are configurable in
`config/default.toml` via `provider_reputation_decay` and
`provider_reputation_retention`.

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
persists to `node-data/state/price_board.v2.bin` on shutdown and every
`price_board_save_interval` seconds. If the file is missing or corrupted the
board starts empty. The persistence path, window size, and save interval are
configurable via `node-data/config.toml`:

```toml
price_board_path = "state/price_board.v2.bin"
price_board_window = 100
price_board_save_interval = 30
```

Each entry is an unsigned 64‑bit integer, so disk usage is `8 * price_board_window`
bytes (≈800 B for the default). Older prices are dropped as new ones arrive.

Clear the board by deleting the persistence file:

```bash
rm node-data/state/price_board.v2.bin
```

Logs emit `loaded price board` and `saved price board` messages, while metrics
`price_band_p25{lane}`, `price_band_median{lane}`, and `price_band_p75{lane}`
track quantile bands. Suggested bids are adjusted by a backlog factor of
`1 + backlog/window` computed per lane, excluding deferred industrial jobs.

> **First-party CRC reminder:** `encode_blob`/`decode_blob` now depend on the
> in-house CRC32 helper exposed from `crypto_suite::hashing::crc32`. The
> current implementation intentionally returns `Err(Unimplemented)` until the
> checksum backend lands, so manual migrations should surface that error instead
> of silently succeeding with stale third-party code.

## Receipt Settlement

Matches between bids and asks produce `Receipt` objects that debit CT from the
buyer and pay the provider. The settlement engine persists balances in a
RocksDB-backed `AccountLedger` (`compute_settlement.db`) so CT and industrial
token accruals survive restarts while remaining compatible with the in-memory
bridge account logic.

```text
{ version: 1, job_id, buyer, provider, quote_price, units, issued_at, idempotency_key, lane }
```

The `idempotency_key` is `BLAKE3(job_id || buyer || provider || quote_price || units ||
version || lane)` and is used as the primary index in the `ReceiptStore`.  Receipts are
persisted via `compare_and_swap` so duplicates are ignored.  On startup the
store reloads existing entries and increments `receipt_corrupt_total` for any
damaged records.

`Settlement` operates in one of three modes:

- **DryRun** – records receipts without moving balances (default).
- **Armed** – after a configured delay, transitions to `Real`.
- **Real** – debits buyers and pays providers in CT.

Operators can toggle modes via CLI and inspect the current state with the
`settlement_status` RPC, which now reports both CT and industrial balances for
the requested provider.  The `compute_market.provider_balances` RPC streams the
same data for explorers, and `compute_market.audit` exposes the recent
ledger events emitted by the durable audit log.

When `Real`, each finalized receipt subtracts `quote_price * units` from the
buyer and accrues the same amount for the provider with an event tag while
recording the delta in the audit log.  Split payments credit both the CT and
industrial ledgers atomically, and `compute_market.recent_roots` returns the
latest settlement Merkle roots for explorer dashboards.  Failures (e.g.,
insufficient funds) are archived and cause the system to revert to `DryRun`.
Metrics track behaviour:

- `settle_applied_total` – receipts successfully debited and paid.
- `settle_failed_total{reason}` – settlement failures.
- `settle_mode_change_total{to}` – mode transitions.
- `matches_total{dry_run,lane}` – receipts processed by the match loop.
- `receipt_persist_fail_total` – database write errors.
- `match_loop_latency_seconds{lane}` – time per match-loop iteration.

The `settlement_cluster` test exercises idempotency by applying receipts across
node restarts and asserting that metrics record exactly one application.

### Lane-aware matching & batching

The matcher maintains per-lane order books and rotates lanes in a
round-robin fairness window so consumer traffic cannot starve industrial jobs
or vice-versa. Orders inside each lane follow strict price-time priority and
are drained in configurable batches. The matcher persists every matched
receipt with its lane tag; on restart the `ReceiptStore` filters out completed
matches and only replays outstanding orders.

Operators can tune behaviour with environment variables:

- `TB_COMPUTE_MATCH_BATCH` – maximum matches drained per loop iteration
  before yielding (default: `32`).
- `TB_COMPUTE_LANE_CAP` – per-lane queue depth cap enforced during seeding
  (default: `1024`).
- `TB_COMPUTE_FAIRNESS_MS` – fairness window per lane before rotation in
  milliseconds (default: `5`).
- `TB_COMPUTE_STARVATION_SECS` – delay before emitting starvation warnings for
  lanes with stuck bids (default: `30`).

`compute_market.stats` now reports `lanes` (per-lane queue depth, oldest
waiting age), `lane_starvation` (active warnings), and `recent_matches` keyed by
lane. The CLI consumes the same payload and renders queue depths, starvation
alerts, and the latest persisted matches alongside the existing backlog
figures.

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

Providers that miss declared job deadlines have their bonds slashed via `penalize_sla`, incrementing `industrial_rejected_total{reason="SLA"}` for dashboard alerts. Operators should set Prometheus rules to page when this counter rises. The scheduler tracks each job's expected versus actual runtime and records overruns in the `compute_job_timeout_total` counter. Jobs resubmitted after a timeout bump `job_resubmitted_total`, aiding SLA enforcement audits.

Providers advertise hardware capabilities—including supported frameworks like CUDA or OpenCL—through offer capabilities. Clients can query a provider's registered hardware via the `compute.provider_hardware` RPC:

```bash
curl localhost:26658 -d '{"jsonrpc":"2.0","id":1,"method":"compute.provider_hardware","params":{"provider":"prov1"}}'
```

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

### Cancellation Edge Cases and Refunds

When a job is cancelled, the courier rolls back any resource reservations via
`release_resources(job_id)` with exponential backoff. If the job finishes while
the cancellation is in flight, the scheduler detects the completed state and
skips the cancellation. Reasons are persisted for audit, and operators can query
state with `compute market status <job_id>`. Refunds honour the original CT
split.

### Querying Admission Parameters

Governance proposals of type `Admission` can retune the budgets at runtime.
Parameter keys map to fields in `governance::Params` and apply via
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
