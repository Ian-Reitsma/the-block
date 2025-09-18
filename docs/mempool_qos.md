# Mempool QoS and Spam Controls

The mempool enforces a rolling fee floor computed from the 75th percentile of recent transaction fees. Transactions below this dynamic threshold trigger a wallet warning and may be rejected. Governance controls both the sampling window and the percentile via the parameters `mempool.fee_floor_window` and `mempool.fee_floor_percentile`. Submitters can propose updates directly from the CLI, e.g. `contract gov param update mempool.fee_floor_window 512`, which creates a proposal bounded by the registry defaults. Every activation (or rollback) appends an entry to `governance/history/fee_floor_policy.json`, increments the `fee_floor_window_changed_total` counter, and is surfaced through the explorer endpoint `/mempool/fee_floor_policy` for operator review.

Each sender is limited to a fixed number of outstanding slots. When the mempool overflows, lowest-fee transactions are evicted first and counted via `mempool_evictions_total`.

`mempool/scoring.rs` contains the reputation-weighted scoring model. The current fee floor is exported as the `fee_floor_current` gauge for telemetry and explorer visualisation.

Admission maintains a per-sender occupancy map keyed by account address. Each new submission acquires a slot before account validation and drops the reservation automatically when the transaction fails validation. Slots are released whenever a transaction is mined, explicitly dropped, or force-evicted so that senders do not become stuck at the `max_pending_per_account` ceiling. The eviction path records the hash of every displaced transaction for auditability; the most recent entries are surfaced through the blockchain API for operators to inspect.

The fee floor is recomputed on every admission using the rolling window. Whenever the percentile shifts, a `tracing` log entry is emitted with the previous and new thresholds so that dashboards can correlate acceptance spikes with fee policy changes. `mempool.stats` now returns the live floor alongside percentile fee and age summaries, allowing downstream tooling to align wallet guidance with the dynamic guardrail. The explorer REST API also serves `/mempool/fee_floor`, providing a ready-to-plot time series derived from the archived `fee_floor_current` metric. Regression coverage lives in `node/tests/mempool_eviction.rs`, which exercises slot caps, hash audit trails, and overflow eviction ordering.

## Wallet guidance and telemetry

The `contract wallet send` flow consumes the same `mempool.stats` endpoint to retrieve the latest floor, caching responses for ten seconds so successive builds avoid redundant RPCs. When the user-provided fee is below the floor the CLI issues a localized warning (English, Spanish, French, German, Portuguese, and Simplified Chinese are supported, selected via `--lang`, `TB_LANG`, or `LANG`) and offers to auto-bump to the floor, force the original fee, or cancel. The `--auto-bump` and `--force` flags provide non-interactive overrides, while `--json` emits a programmatic envelope with the proposed payload, floor, and decision so automated tooling can react deterministically. Example JSON output:

```json
{
  "status": "ready",
  "user_fee": 2,
  "effective_fee": 10,
  "fee_floor": 10,
  "lane": "consumer",
  "warnings": ["Warning: fee 2 is below the consumer fee floor (10)."],
  "auto_bumped": true,
  "forced": false,
  "payload": {"from_": "...", "to": "...", "fee": 10, "nonce": 0, "pct_ct": 100, "amount_consumer": 100, "amount_industrial": 0, "memo": []}
}
```

Every warning or override is reported back to the node through the `mempool.qos_event` RPC, ensuring telemetry tracks wallet-side pressure. The counters `fee_floor_warning_total{lane}` and `fee_floor_override_total{lane}` distinguish between simple guidance and forced sends, complementing the existing `fee_floor_current` gauge. Operators can therefore correlate spikes in overrides with subsequent rejection rates and governance discussions.

The wallet RPC client now inspects the JSON-RPC envelope returned by `mempool.qos_event`, surfacing transport failures, server-side errors, and acknowledgements whose `status` field is not `"ok"`. CLI users see explicit error messages when telemetry is rejected, and JSON-mode consumers can rely on the exit status to detect failures programmatically.
