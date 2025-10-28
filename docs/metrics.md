# Telemetry Metrics
> **Review (2025-12-14):** Synced telemetry guidance with the first-party monitoring rollout and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-29).

The node exposes internal counters via a minimal HTTP exporter when compiled
with the `telemetry` feature. Start a node with the `--metrics-addr` flag and
visit the `/metrics` endpoint to retrieve the first-party text snapshot served by
`runtime::telemetry`.

```bash
$ cargo run --bin node --features telemetry -- run --metrics-addr 127.0.0.1:9100
```

Sample output:

```bash
$ curl -s http://127.0.0.1:9100/metrics | head -n 5
# HELP tx_submitted_total Total submitted transactions
# TYPE tx_submitted_total counter
tx_submitted_total 0
# HELP block_mined_total Total mined blocks
# TYPE block_mined_total counter
```

The exporter currently tracks:

- `tx_submitted_total` – transactions submitted to the mempool
- `tx_rejected_total{reason}` – transactions rejected with a labeled reason
- `block_mined_total` – blocks successfully mined
- `mempool_size{lane}` – gauge of current mempool size per fee lane
- `consumer_fee_p50` / `consumer_fee_p90` – sampled consumer fees
- `industrial_rejected_total{reason}` – industrial transactions dropped or deferred
- `industrial_rejected_total{reason="SLA"}` – provider slashed for missing deadlines
- `admission_mode{mode}` – comfort guard state
- `gossip_duplicate_total` – hashes ignored due to TTL deduplication
- `gossip_fanout_gauge` – number of peers each gossip message relays to
- `subsidy_bytes_total{type}` – bytes eligible for CT subsidy per class
- `subsidy_cpu_ms_total` – compute time eligible for CT subsidy
- `read_denied_total{reason}` – reads rejected due to rate limits
- `storage_repair_bytes_total` / `storage_repair_failures_total` – bytes reconstructed and failed repairs
- `storage_chunk_size_bytes` – distribution of chunk sizes written during uploads
- `storage_put_chunk_seconds` – time taken to store individual chunks
- `storage_provider_rtt_ms` – observed storage provider round-trip time
- `storage_provider_loss_rate` – observed storage provider loss rate
- `gov_votes_total` / `gov_activation_total` / `gov_rollback_total` – governance
  vote, activation, and rollback counters
- `gov_activation_delay_seconds` – time between proposal commit and activation
- `gov_open_proposals` / `gov_quorum_required` – gauges for governance state
- `storage_initial_chunk_size` / `storage_final_chunk_size` – first and last chunk sizes per object
- `storage_put_eta_seconds` – estimated total upload time for the current object
- `settle_applied_total` – receipts successfully debited and paid
- `settle_failed_total{reason}` – settlement failures by reason
- `settle_mode_change_total{to}` – settlement mode transitions
- `settle_audit_mismatch_total` – settlement audit discrepancies detected
- `explorer_block_payout_read_total{role}` /
  `explorer_block_payout_ad_total{role}` /
  `explorer_block_payout_ad_it_total{role}` – explorer-reported per-role read
  subsidy and dual-token advertising payouts cached by the metrics aggregator,
  updated whenever ingest observes a higher total for a given role
- `treasury_disbursement_count` – total staged or executed treasury disbursements
  observed by the aggregator (monotonic across restarts)
- `treasury_disbursement_amount_ct` /
  `treasury_disbursement_amount_it` – cumulative CT and IT amounts released by
  governance disbursements
- `treasury_balance_current_ct` /
  `treasury_balance_current_it` – latest CT/IT balances pulled from the
  governance store or legacy JSON snapshot
- `treasury_balance_last_delta_ct` /
  `treasury_balance_last_delta_it` – most recent CT/IT balance change recorded in
  the history feed (positive for accruals, negative for executions)
- `ad_readiness_total_usd_micros` / `ad_readiness_settlement_count` /
  `ad_readiness_ct_price_usd_micros` / `ad_readiness_it_price_usd_micros` –
  readiness-window USD totals, settlement counts, and oracle prices published by
  the ad readiness pipeline and mirrored into dashboards and CI artefacts
- `ttl_drop_total` / `startup_ttl_drop_total` – messages dropped due to TTL expiry during runtime and startup
- `orphan_sweep_total` – orphan blocks removed during periodic sweeps
- `snapshot_duration_seconds` / `snapshot_fail_total` – snapshot round-trip time and failure counts
- `snapshot_interval` / `snapshot_interval_changed` – active snapshot interval and gauge of interval changes
- `param_change_pending{key}` – governance parameter changes queued for activation
- `param_change_active{key}` – current active governance parameter values
- `synthetic_convergence_seconds` – end-to-end probe duration emitted by scripts/synthetic.sh
- `synthetic_success_total` – successful synthetic runs
- `synthetic_fail_total{step}` – probe failures by step
- `peer_handshake_failure_total{reason}` – failed peer handshakes during bootstrap

For a full list of counters, see `src/telemetry.rs`.
