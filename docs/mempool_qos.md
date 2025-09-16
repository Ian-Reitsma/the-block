# Mempool QoS and Spam Controls

The mempool enforces a rolling fee floor computed from the 75th percentile of recent transaction fees. Transactions below this dynamic threshold trigger a wallet warning and may be rejected.

Each sender is limited to a fixed number of outstanding slots. When the mempool overflows, lowest-fee transactions are evicted first and counted via `mempool_evictions_total`.

`mempool/scoring.rs` contains the reputation-weighted scoring model. The current fee floor is exported as the `fee_floor_current` gauge for telemetry and explorer visualisation.

Admission maintains a per-sender occupancy map keyed by account address. Each new submission acquires a slot before account validation and drops the reservation automatically when the transaction fails validation. Slots are released whenever a transaction is mined, explicitly dropped, or force-evicted so that senders do not become stuck at the `max_pending_per_account` ceiling. The eviction path records the hash of every displaced transaction for auditability; the most recent entries are surfaced through the blockchain API for operators to inspect.

The fee floor is recomputed on every admission using the rolling window. Whenever the percentile shifts, a `tracing` log entry is emitted with the previous and new thresholds so that dashboards can correlate acceptance spikes with fee policy changes. `mempool.stats` now returns the live floor alongside percentile fee and age summaries, allowing downstream tooling to align wallet guidance with the dynamic guardrail. The explorer REST API also serves `/mempool/fee_floor`, providing a ready-to-plot time series derived from the archived `fee_floor_current` metric. Regression coverage lives in `node/tests/mempool_eviction.rs`, which exercises slot caps, hash audit trails, and overflow eviction ordering.
