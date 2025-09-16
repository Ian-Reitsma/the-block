# Telemetry Operations Runbook

This guide captures the day-two playbook for responding to telemetry alerts and
for correlating metrics with structured logs across the fleet.

## Golden Signals & Alert Sources

- **Release governance** – Alert on `release_quorum_fail_total` deltas without a
  matching signer update in the governance DB. The main Grafana dashboard
  includes a panel sourced from `monitoring/grafana/telemetry.json` that tracks
  quorum health alongside `release_installs_total` so lagging installs surface
  quickly.
- **QUIC stability** – Watch `quic_handshake_fail_total{peer}` and
  `quic_retransmit_total{peer}`. The metrics aggregator automatically requests a
  `/logs/search` dump when either counter spikes and stores the payload under
  `$TB_LOG_DUMP_DIR`. Grafana panels link directly to the cached diagnostics
  returned by the `net.quic_stats` RPC.
- **Compute marketplace** – The `compute_market_dashboard.json` add-on highlights
  `fee_floor_current`, per-sender slot pressure, and SLA violation counters so
  admission policy or scheduler regressions trigger fast follow-up.
- **Correlating anomalies** – `log_correlation_fail_total{metric}` increments
  whenever the aggregator cannot locate logs for a metric spike. Combine this
  with `aggregator_ingest_total{result="error"}` to detect ingest backpressure.

## Investigating Alerts

1. **Confirm scope** using `contract logs correlate-metric --metric <name>` to
   pull the cached log excerpts for the alerting metric. Supply `--rows` to cap
   noisy floods and `--max-correlations` when a spike involves multiple peers.
2. **Drill into per-peer health** via `contract-cli net quic-stats --json` to
   inspect retransmits, endpoint reuse, and cached handshake latency. Authenticate
   with `--token <ADMIN>` when calling remote nodes.
3. **Review release state** by hitting the explorer
   `GET /releases?page=0&page_size=20` endpoint (or the CLI equivalent) to check
   whether lagging nodes correlate with new release approvals or partial signer
   churn.
4. **Validate log ingest** with `GET /logs/search?since=<ts>&limit=100`. When
   the response is empty but the alert persists, rotate the indexer encryption key
   via `contract logs rotate-key` to ensure stale passphrases are not blocking
   decrypts.

## Preventive Maintenance

- Rotate aggregator auth tokens quarterly and ensure nodes reload them without a
  restart (`metrics_aggregator.auth_token` hot-reloads).
- Run `scripts/log_indexer_load.sh` monthly to benchmark ingest headroom and to
  confirm prepared statements still batch inserts as expected.
- Schedule `sim/log_correlation.rs` and `sim/lagging_release.rs` in CI to test
  correlation coverage and staged rollout behaviour before deploying signer or
  mirror changes.
- Capture quarterly snapshots of `monitoring/grafana/telemetry.json` so alerting
  thresholds stay version-controlled alongside the metrics schema.

Maintain SLA targets by responding to red alerts within five minutes and keeping
exporter availability above 99%. Document every incident in the change log with
links to the correlated metrics and log dumps for future audits.
