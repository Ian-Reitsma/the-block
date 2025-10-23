# Monitoring Dashboards
> **Review (2025-09-25):** Synced Monitoring Dashboards guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This directory contains subsystem-specific Grafana dashboards that complement
the primary telemetry bundle under `monitoring/grafana/`. Each JSON file is
ready to import once the foundation telemetry stack is running.

- `compute_market_dashboard.json` visualises backlog factors, fee-floor
  enforcement, courier retry behaviour, and the SLA violation counters alongside
  the rolling `fee_floor_current` gauge and the companion
  `fee_floor_warning_total{lane}`/`fee_floor_override_total{lane}` counters so
  operators can compare pricing policy with realised demand. Governance changes
  to `mempool.fee_floor_window` and `mempool.fee_floor_percentile` increment
  `fee_floor_window_changed_total` and surface in the same dashboard.
- The consolidated bridge row now ships in every core dashboard. Panels chart
  five-minute deltas for `bridge_reward_claims_total`,
  `bridge_reward_approvals_consumed_total`,
  `bridge_settlement_results_total{result,reason}`, and
  `bridge_dispute_outcomes_total{kind,outcome}` so operators can audit reward
  consumption, settlement results, and dispute resolutions without importing
  third-party widgets. Additional panels plot
  `bridge_liquidity_locked_total{asset}`,
  `bridge_liquidity_unlocked_total{asset}`,
  `bridge_liquidity_minted_total{asset}`, and
  `bridge_liquidity_burned_total{asset}` to surface cross-chain liquidity flow.
  Remediation coverage now spans three panels: one renders
  `sum by (action, playbook)(increase(bridge_remediation_action_total[5m]))` to
  display the recommended playbook alongside each anomaly, a second charts
  `sum by (action, playbook, target, status)(increase(bridge_remediation_dispatch_total[5m]))`
  so dispatch successes, skips, and failures per target stay visible without
  leaving the dashboard, and the new acknowledgement panel tracks
  `sum by (action, playbook, target, state)(increase(bridge_remediation_dispatch_ack_total[5m]))`
  so operators can confirm downstream paging/governance hooks acknowledged or
  closed out each playbook.
- The metrics aggregator now exposes a `/anomalies/bridge` endpoint alongside the
  bridge row. A rolling detector keeps a per-peer baseline for the reward,
  approval, settlement, and dispute counters, increments
  `bridge_anomaly_total` when a spike exceeds the configured threshold, and the
  dashboards plot five-minute increases so operators can correlate alerts with
  the underlying counters directly from the first-party panels.
- Companion gauges
  `bridge_metric_delta{metric,peer,labels}` and
  `bridge_metric_rate_per_second{metric,peer,labels}` stream the detector’s
  raw deltas and per-second growth so dashboards can overlay anomaly events with
  the observed counter velocity for each relayer/label tuple. The `bridge`
  alert group queries these gauges to surface `BridgeCounterDeltaSkew`,
  `BridgeCounterRateSkew`, and the label-aware companions
  `BridgeCounterDeltaLabelSkew`/`BridgeCounterRateLabelSkew` when a relayer’s
  aggregate or per-label growth exceeds three times the 30-minute average.
  Baselines persist across restarts, and labelled anomalies now feed the
  remediation engine: `/remediation/bridge` lists the persisted page, throttle,
  quarantine, or escalation actions while
  `bridge_remediation_action_total{action,playbook}` exposes both the action and
  the follow-up playbook for dashboards and alert runbooks.
- The remediation engine dispatches every action to first-party hooks. Configure
  `TB_REMEDIATION_*_URLS` for HTTPS targets or `TB_REMEDIATION_*_DIRS` for spool
  directories to receive the structured JSON payload (peer id, metric, labels,
  playbook, `annotation`, `dashboard_panels`, `response_sequence`, and
  `dispatched_at`). Each attempt increments
  `bridge_remediation_dispatch_total{action,playbook,target,status}` **and**
  records acknowledgement state via
  `bridge_remediation_dispatch_ack_total{action,playbook,target,state}`, then
  appends to `/remediation/bridge/dispatches`, letting dashboards and alerting
  policy flag skipped hooks, failing endpoints, or unacknowledged escalations
  without scraping Prometheus.
- Automated follow-ups now run entirely within the aggregator. Pending actions
  persist `dispatch_attempts`, `auto_retry_count`, retry timestamps, and
  follow-up notes so the engine can queue deterministic retries and governance
  escalations when policy thresholds expire. The acknowledgement parser accepts
  plain-text hook responses as well as JSON, mapping strings like `"ack pager"`
  or `"closed: resolved"` into structured records. New alerts—
  `BridgeRemediationAckPending` and `BridgeRemediationClosureMissing`—read the
  persisted acknowledgement counter and page when hooks stall or never close,
  rounding out the first-party paging/escalation loop.
- The CI-run `bridge-alert-validator` binary now drives the shared
  `alert_validator` module, replaying canned datasets for the bridge,
  chain-health, dependency-registry, and treasury alert groups so expression
  changes require first-party coverage instead of promtool fixtures.
- Bridge fixtures now cover recovery tails and partial windows—including
  dispute outcomes and quorum-failure approvals—so `BridgeCounter*Skew`
  heuristics stay quiet while anomalies cool down or when fewer than six samples
  exist.
- Extend dashboards with `did_anchor_total` to monitor identifier churn; the
  explorer exposes `/dids/metrics/anchor_rate` and `/dids` for recent history so
  panels can link directly to the underlying records.

The consolidated cluster dashboard that ships with the repo lives at
`monitoring/grafana/telemetry.json`. It already exposes governance rollout
metrics (`release_quorum_fail_total`, `release_installs_total`), QUIC diagnostics
with per-peer retransmit and handshake panels, and log-correlation alerts fed by
the metrics aggregator.

Import any of these dashboards after running `make monitor` (or the native
equivalent) and ensure nodes start with `--metrics-addr` and
`--features telemetry`. Dashboards assume the runtime exporter on `localhost:9090`; edit the
embedded `datasource` block if your deployment differs.
