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
  A new "Read Ack Outcomes (5m delta)" panel breaks down
  `read_ack_processed_total{result="ok|invalid_signature|invalid_privacy"}` so
  the fresh `invalid_privacy` label is visible without digging through raw
  PromQL.
- The explorer block-payout row pulls straight from
  `explorer_block_payout_read_total{role}` and
  `explorer_block_payout_ad_total{role}`. The metrics aggregator now caches the
  most recent role totals per explorer peer and only increments the counter
  handles when a higher value arrives, so the Prometheus/Grafana panels chart
  live deltas without double counting. Complementary gauges
  `explorer_block_payout_read_last_seen_timestamp{role}` and
  `explorer_block_payout_ad_last_seen_timestamp{role}` record the Unix timestamp
  of the latest increment, enabling the
  `ExplorerReadPayoutStalled`/`ExplorerAdPayoutStalled` alerts to fire when a
  role stays flat for thirty minutes after producing non-zero totals. Integration
  tests ingest successive payloads and verify both the counters and gauges
  advance on the second scrape, matching the `increase()` queries baked into the
  dashboard.
- The consolidated bridge row now ships in every core dashboard. Panels chart
  five-minute deltas for `bridge_reward_claims_total`,
  `bridge_reward_approvals_consumed_total`,
  `bridge_settlement_results_total{result,reason}`, and
  `bridge_dispute_outcomes_total{kind,outcome}` so operators can audit reward
  consumption, settlement results, and dispute resolutions without importing
  third-party widgets. `dashboards_include_bridge_counter_panels` now parses each
  generated Grafana JSON (dashboard/operator/telemetry/dev) to ensure those
  reward-claim, approval, settlement, and dispute panels retain their queries and
  legends across templates. `dashboards_include_bridge_remediation_legends_and_tooltips` keeps the remediation row legends/descriptions in lockstep across templates so tooltips stay aligned with the PromQL. Additional panels plot
  `bridge_liquidity_locked_total{asset}`,
  `bridge_liquidity_unlocked_total{asset}`,
  `bridge_liquidity_minted_total{asset}`, and
  `bridge_liquidity_burned_total{asset}` to surface cross-chain liquidity flow.
  Remediation coverage now spans four panels: one renders
  `sum by (action, playbook)(increase(bridge_remediation_action_total[5m]))` to
  display the recommended playbook alongside each anomaly, a second charts
  `sum by (action, playbook, target, status)(increase(bridge_remediation_dispatch_total[5m]))`
  so dispatch successes, skips, and failures per target stay visible without
  leaving the dashboard, a third acknowledgement counter panel tracks
  `sum by (action, playbook, target, state)(increase(bridge_remediation_dispatch_ack_total[5m]))`
  so operators can confirm downstream paging/governance hooks acknowledged or
  closed out each playbook, and a latency histogram overlays p50/p95 curves from
  `bridge_remediation_ack_latency_seconds{playbook,state}` plus the policy gauge
  `bridge_remediation_ack_target_seconds{playbook,policy}` so slow closures stand
  out against the configured retry/escalation thresholds before policy windows
  expire.
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
  without scraping Prometheus. The bridge row also charts the
  `bridge_remediation_spool_artifacts` gauge so responders can see outstanding
  on-disk payloads without tailing the spool directory. Regression tests now use
  a `RemediationSpoolSandbox` helper to fabricate and clean up temp directories
  per scenario, enabling page/throttle/quarantine/escalate directories and
  exercising `remediation_spool_sandbox_restores_environment` so `/tmp` hygiene
  and env guards (`TB_REMEDIATION_*_DIRS`) stay entirely first party.
- Automated follow-ups now run entirely within the aggregator. Pending actions
  persist `dispatch_attempts`, `auto_retry_count`, retry timestamps, and
  follow-up notes so the engine can queue deterministic retries and governance
  escalations when policy thresholds expire. The acknowledgement parser accepts
  plain-text hook responses as well as JSON, mapping strings like `"ack pager"`
  or `"closed: resolved"` into structured records. Retry and escalation windows
  respect `TB_REMEDIATION_ACK_RETRY_SECS`, `_ESCALATE_SECS`, and `_MAX_RETRIES`
  defaults plus suffix overrides such as
  `TB_REMEDIATION_ACK_RETRY_SECS_GOVERNANCE_ESCALATION` for playbook-specific
  tuning. Completion latency feeds the
  `bridge_remediation_ack_latency_seconds{playbook,state}` histogram, which now
  persists samples across restarts and drives the dashboard panel alongside the
  policy gauge. New alerts—`BridgeRemediationAckPending`,
  `BridgeRemediationClosureMissing`, and `BridgeRemediationAckLatencyHigh`—read
  the persisted metrics to page when hooks stall, never close, or exceed the
  configured p95 policy target, rounding out the first-party paging/escalation
  loop.
- The WAN chaos dashboards ingest the new
  `chaos_readiness{module,scenario}` gauge and `chaos_sla_breach_total` counter
  emitted by the metrics aggregator after verifying `/chaos/attest` payloads.
  Signed artefacts generated by `sim/chaos_lab.rs` can be posted directly to the
  aggregator, and operators can query `/chaos/status` for the latest readiness
  snapshot. CI recipes (`just chaos-suite`, `cargo xtask chaos`) run the same
  first-party binaries, ensuring clusters ship readiness attestation coverage
  without third-party tooling. The auto-generated Grafana dashboard now includes
  a dedicated **Chaos** row visualising readiness and five-minute breach deltas,
  the `chaos_lab_attestations_flow_through_status` regression keeps the HTTP
  ingest path hermetic by exercising `/chaos/attest` end-to-end with the
  first-party simulation crate, and `chaos_attestation_rejects_invalid_signature`
  tampers with payloads to ensure forged artefacts never update readiness
  gauges.
- Use `contract remediation bridge --aggregator http://localhost:9000 --limit 5`
  during incidents to print the persisted actions, retry history, follow-up
  notes, acknowledgement timestamps, and dispatch log straight from the
  aggregator without relying on external tooling. Filter the output with
  `--playbook` or `--peer` when triaging a specific workflow, and pass `--json`
  to stream the same data to automation without leaving the first-party binary.
  The CLI consumes the same JSON endpoints that power the dashboards and keeps
  everything first party.
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
