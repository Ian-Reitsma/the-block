# Monitoring
> **Review (2025-12-14):** Reaffirmed runtime HTTP client coverage, noted the aggregator/gateway server migration outstanding, and reconfirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-29).

The default monitoring stack now ships entirely with first-party components. A
lightweight dashboard generator polls the node’s telemetry exporter and renders
HTML summaries grouped by subsystem. Operators no longer need Prometheus or
Grafana; the tooling relies exclusively on `runtime::telemetry` snapshots and
the shared `monitoring/metrics.json` catalogue.

Operational alert handling and correlation procedures live in the
[`Telemetry Operations Runbook`](telemetry_ops.md).

## Quick start

Docker (default):

```bash
make monitor
```

To run the stack in the background (as used by `scripts/bootstrap.sh`):

```bash
DETACH=1 make monitor
```

Native (no Docker or `--native-monitor`):

```bash
scripts/monitor_native.sh
```

`make monitor --native-monitor` and `./scripts/bootstrap.sh --native-monitor` call the
same script. When Docker isn't installed or the daemon is stopped, these
commands automatically fall back to the native binaries.

The native script wraps the new `cargo run --bin snapshot` utility, streaming
the node’s `/metrics` endpoint, regenerating `monitoring/output/index.html`
every few seconds, and serving it on <http://127.0.0.1:8088>. Set
`TB_MONITORING_ENDPOINT` to track a remote node or adjust
`TB_MONITORING_OUTPUT`/`FOUNDATION_DASHBOARD_PORT` to suit local requirements.
When TLS staging manifests are available, the script also respects
`TB_MONITORING_TLS_*` prefixes so operators can reuse the same certificates the
node consumes. `monitoring/prometheus.yml` now documents canonical telemetry
targets for clusters and the Docker compose recipe consumes it as a bind mount.

The rendered dashboard mirrors the previous Grafana panels: per-lane mempool
size, banned peers, gossip duplicate counts, subsidy gauges
(`subsidy_bytes_total{type}`, `subsidy_cpu_ms_total`, `rent_escrow_locked_ct_total`),
per-peer request/drop panels from `peer_request_total`/`peer_drop_total{reason}`,
scheduler match histograms (`scheduler_match_total{result}`,
`scheduler_provider_reputation`) and log size statistics derived from the
`log_size_bytes` histogram. Fee-floor cards combine `fee_floor_current` with
`fee_floor_warning_total{lane}`/`fee_floor_override_total{lane}` so operators can
trace wallet guidance, and DID anchor sections plot `did_anchor_total` alongside
recent `/dids` history for cross-navigation. Additional gauges expose
`subsidy_auto_reduced_total` and `kill_switch_trigger_total` so operators can
correlate reward shifts with governance interventions. The HTML refreshes every
five seconds by default and never leaves first-party code.

Benchmarks surface in the same view. When suites export Prometheus samples via
`TB_BENCH_PROM_PATH`, the generator acquires the shared lock, ingests
`benchmark_ann_soft_intent_verification_seconds` alongside the live metrics, and
renders a dedicated **Benchmarks** row in both Grafana and the HTML dashboard.
Panels plot the ANN verification latency and annotate recent runs so operators
can correlate wallet-scale ANN timings with gateway pacing guidance without
leaving the in-house tooling; larger badge tables and wallet-supplied entropy
values now show up in the time series without clobbering concurrent runs.

## Chaos attestations and readiness metrics

The WAN chaos verifier now lives entirely inside the workspace:

- `sim/src/chaos.rs` models overlay, storage, and compute scenarios with
  deterministic recovery curves. The `chaos_lab` binary (`cargo run -p tb-sim
  --bin chaos_lab`) drives the harness, exports CSV snapshots, and signs one
  attestation per module using an Ed25519 key (automatically generated when
  `TB_CHAOS_SIGNING_KEY` is absent).
- `monitoring/src/chaos.rs` defines the attestation codecs and verification
  helpers the aggregator consumes. Drafts clamp readiness/SLA scores into `[0,1]`
  before hashing, and signed payloads record the scenario, module, readiness,
  breach count, observation window, and verifying key fingerprint.
- `metrics-aggregator` ingests the signed payloads through `/chaos/attest`,
  persists the latest readiness snapshot per `(scenario,module)` pair, and
  exposes `/chaos/status` for downstream tooling. Accepted payloads update the
  metrics `chaos_readiness{module,scenario}` and
  `chaos_sla_breach_total`, and now mirror per-site readiness via
  `chaos_site_readiness{module,scenario,site,provider}` so dashboards and
  automation can diff provider-specific regressions. The handler sorts site
  entries, prunes stale label handles when scenarios drop a site/provider pair,
  and emits a `chaos_status_tracker_poisoned_recovering` warning if it has to
  recover from a poisoned readiness mutex, keeping JSON/Grafana snapshots stable
  even when previous runs panicked mid-update.
- A dedicated regression (`chaos_attestation_round_trip`) posts the `chaos_lab`
  output into `/chaos/attest`, verifies the `/chaos/status` response, and
  exercises out-of-range, digest-mismatch, malformed-module, and
  signature-tampering paths so invalid attestations never poison the readiness
  cache. A second end-to-end test
  (`chaos_lab_attestations_flow_through_status`) drives the actual
  `chaos_lab` artefacts through the HTTP handler using only first-party crates
  (`tb-sim` as a dev-dependency), asserting the returned status payload and the
  `chaos_readiness`/`chaos_sla_breach_total` updates plus signer digests.
- CI wires the harness into `just chaos-suite` and `xtask chaos`, ensuring
  release tags only advance once the chaos scenarios complete and emit the new
  readiness metrics.

`chaos_lab` can also fetch an existing `/chaos/status` baseline when operators set
`TB_CHAOS_STATUS_ENDPOINT`. The binary uses `httpd::BlockingClient` with a
10-second timeout, decodes the payload via
`foundation_serialization::json::Value`, and emits both the baseline snapshot
(`TB_CHAOS_STATUS_BASELINE`) and a diff against the newly captured attestations
(`TB_CHAOS_STATUS_DIFF`). Setting `TB_CHAOS_REQUIRE_DIFF=1` turns empty diffs into
hard failures so soak automation cannot silently accept regressions. Per-site
readiness rows are written to `TB_CHAOS_OVERLAY_READINESS` and contain the
scenario, module, site, provider, scenario readiness, current readiness, and
baseline deltas; they stay on the first-party codecs and power downstream
automation. Setting `TB_CHAOS_PROVIDER_FAILOVER` captures
`chaos_provider_failover.json`, which enumerates per-provider failover drills,
recomputes scenario readiness, and fails the run when an outage does not drop
readiness or surface a diff entry.

`cargo xtask chaos` consumes those artefacts through the same
`foundation_serialization` facade, summarising module totals, scenario readiness,
readiness improvements/regressions, provider churn, duplicate scenario/site pairs,
and the provider failover matrix with pure `std` collections. The command now
enforces release gating by failing when overlay readiness drops, sites disappear,
or provider failover drills do not register diffs, printing the captured diff
count alongside per-scenario readiness transitions so release managers can review
everything without external dashboards. It also prints the BLAKE3 digest and byte
length for the manifest and bundle plus the mirrored filesystem paths or derived
S3 object keys so operators and CI logs can audit archives without opening JSON
blobs. Uploads ride the bespoke `foundation_object_store` client, which now ships
a canonical-request regression and blocking upload harness that prove AWS Signature
V4 headers match the published examples while honouring `TB_CHAOS_ARCHIVE_RETRIES`
(minimum 1) and optional `TB_CHAOS_ARCHIVE_FIXED_TIME` timestamps for reproducible
signatures.

The release tooling now bakes this gate into the provenance workflow.  Before
hashing build artefacts, `scripts/release_provenance.sh` shells out to
`cargo xtask chaos --out-dir releases/<tag>/chaos`, forwarding any
`TB_CHAOS_*` environment overrides and refusing to proceed until the
snapshot/diff/overlay/failover JSON payloads all exist and pass the same
regression checks.  `scripts/verify_release.sh` mirrors that contract by failing
whenever the published archive lacks the `chaos/` directory or any of the four
artefacts, so downstream consumers can trust that every signed release exercised
the provider failover matrix.  The `just chaos-suite` recipe now emits the same
artefacts locally (attestations, status snapshot, diff, overlay readiness, and
provider failover) so operators rehearsing the flow by hand observe identical
files.

The Grafana and HTML dashboards now include a dedicated **Chaos** row that
charts `chaos_readiness{module,scenario}`, `chaos_site_readiness{module,site,provider}`,
and the five-minute delta of `chaos_sla_breach_total`. Automation runbooks
continue to reference `/chaos/status` for human-readable snapshots, and
`chaos_lab` persists provider-aware diff artefacts so soak automation can alert
on churn. Signed attestation archives now live under `monitoring/output/chaos/`
when operators set `TB_CHAOS_ATTESTATIONS` during `make monitor`, keeping the
historical artefacts inside first-party storage.

When filesystem scratch space disappears mid-test, the gossip relay shard cache
falls back to an in-memory store, and the peer metrics persistence layer skips
flushes while logging clock rollback warnings. Operators still get readiness
updates, but no panic escapes into CI or long-running chaos rehearsals.

The dashboard generator now inserts a “Block Payouts” row ahead of the bridge
section. Panels chart `sum by (role)(increase(explorer_block_payout_read_total[5m]))`,
`sum by (role)(increase(explorer_block_payout_ad_total[5m]))`, and
`sum by (role)(increase(explorer_block_payout_ad_it_total[5m]))`, rendering the
per-role read-subsidy, consumer-token advertising, and industrial-token
advertising totals mined from block headers so operations can compare
viewer/host/hardware/verifier/liquidity/miner splits without scraping SQLite
directly. Additional single-stat panels surface `explorer_block_payout_ad_usd_total`,
`explorer_block_payout_ad_settlement_count`, and the CT/IT oracle gauges reported by
each explorer peer so CI and dashboards trend the USD spend and conversion rates
alongside the token totals. Legends remain enabled by default, letting operators
focus on specific roles or compare read versus dual-token advertising flows in the
same pane. The shared conversion helper now carries debug assertions and an
uneven-price regression to guarantee these CT/IT totals stay within their
governance budgets, preventing dashboards from ever reflecting double-counted
liquidity.

A neighbouring **Treasury Execution Timeline** panel renders the new
`Block::treasury_events` vector. Each disbursement lists the execution height,
beneficiary, currency, USD amount, and originating transaction hash, giving
operators an at-a-glance ledger of treasury activity without scraping explorer
SQL. The CLI and explorer surfaces expose the same events via first-party codecs,
and the monitoring bundle mirrors them so treasury, governance, and settlement
audits all reference identical data.

Adjacent single-stat tiles now publish
`treasury_disbursement_amount_{ct,it}`, `treasury_disbursement_count`,
`treasury_balance_current_{ct,it}`, and
`treasury_balance_last_delta_{ct,it}` directly from the metrics aggregator. The
reset logic was updated alongside the dual-token gauges so dashboards, CI HTML
snapshots, and runbooks inherit the CT/IT balance story without waiting for the
governance activation. Alerts reuse the same counters to warn when balances or
disbursement volumes drift across currencies.

An “Ad Readiness” row now accompanies the payouts panels. Gauges plot the latest
`ad_readiness_ready`, `ad_readiness_unique_viewers`, `ad_readiness_host_count`,
`ad_readiness_provider_count`, `ad_readiness_total_usd_micros`,
`ad_readiness_settlement_count`, `ad_readiness_ct_price_usd_micros`,
`ad_readiness_it_price_usd_micros`, and the mirrored marketplace oracles
(`ad_readiness_market_{ct,it}_price_usd_micros`) alongside the configured
minimums, while a table lists the active blockers surfaced by
`ad_market.readiness`. Companion panels render
`ad_readiness_utilization_{observed,target,delta}_ppm` grouped by
domain/provider/badge so operators can spot cohorts falling below their target
utilisation even when demand stays constant. The readiness table also renders the
RPC’s `utilization` map so per-cohort prices and ppm deltas sit next to the
aggregate gauges, and the HTML snapshot mirrors the same layout to keep
FIRST_PARTY_ONLY monitoring aligned with the Grafana templates. A counter panel
charts `increase(ad_readiness_skipped_total[5m])` by reason so operators can
spot insufficient viewer/host/provider diversity before enabling the ad rail.

The same row now links directly into the pacing cards. Selection receipts list
the `resource_floor_breakdown` that cleared each auction, and the pacing panels
plot `ad_budget_summary_value{metric}` alongside the deltas derived from
`BudgetBrokerPacingDelta`, letting operators confirm partial snapshots merged
correctly before telemetry exported the metrics—all while staying on the
first-party Prometheus helpers.

The Grafana bundle also dedicates a full **Advertising** row to proof integrity
and pacing. Panels chart five-minute deltas of
`ad_selection_attestation_total{kind,result,reason}`,
`ad_selection_proof_verify_seconds{circuit}`, and
`ad_selection_attestation_commitment_bytes{kind}` so SNARK throughput,
fallbacks, and commitment sizes stay observable. Companion graphs render
`ad_budget_progress{campaign}`, `ad_budget_shadow_price{campaign}`, and
`ad_budget_kappa_gradient{campaign,...}` to expose κ shading, gradient pressure,
and dual-price convergence. A dedicated panel breaks out
`ad_resource_floor_component_usd{component}` so bandwidth, verifier, and host
costs remain visible when demand clears near the floor. The alerts—
`SelectionProofSnarkFallback`, `SelectionProofRejectionSpike`,
`AdBudgetProgressFlat`, and `AdResourceFloorVerifierDrift`—are annotated directly
on the panels when proof supply degrades, pacing stalls, or verifier amortisation
drifts. All panels flow through the helper builders in
`monitoring/src/dashboard.rs`, keeping the JSON templates entirely first party.

Privacy and uplift telemetry chart beside the pacing graphs. Counters
`ad_privacy_budget_total{family,result}` and gauges
`ad_privacy_budget_remaining{family,metric}` highlight badge families approaching
their `(ε, δ)` ceilings or entering cooldown, while `ad_uplift_propensity{sample}`
and `ad_uplift_lift_ppm{impressions}` plot the cross-fitted doubly-robust lift
estimates so calibration drift is visible without replaying training logs. Alert
rules fire when revoked/cooling totals spike or propensity deviates sharply,
keeping privacy abuse and model regressions as observable as pacing stalls.

Prometheus now fires `AdReadinessUtilizationDelta` whenever
`abs(delta_utilization_ppm)` breaches the configured threshold despite steady
request volume. The alert routes to the existing CI/on-call channels, tying the
governance utilisation targets to telemetry and paging operators before cohort
drift forces emergency settlement overrides.

Runbooks and CI artefacts have been updated to expect the delta map; the
`monitoring/src/alert_validator.rs` dataset now covers the new readiness
families so rule edits fail fast if the delta gauges disappear. Dashboard
snapshots, JSON exports, and release checks consume the same
`ad_readiness_utilization_delta_ppm` data, replacing the deprecated legacy
summary panels.

The metrics aggregator now persists the explorer payout counters per peer and
role so deltas remain monotonic across scrapes.
`AppState::record_explorer_payout_metric` tracks a `(peer_id, role)` cache and
only updates the `CounterVec` handles when a new total exceeds the previously
seen sample. It also seeds zero-value handles on startup and writes the current
Unix timestamp to
`explorer_block_payout_{read,ad}_last_seen_timestamp{role}` whenever a role
advances, giving Prometheus a direct way to measure staleness via
`time() - gauge`. The integration suite ingests two payloads and asserts that
`explorer_block_payout_read_total`, `_ad_total`, and `_ad_it_total` report the
latest totals on the second `/metrics` scrape and that the last-seen gauges bump
alongside the deltas, guaranteeing the Grafana row and Prometheus assertions
continue to plot live data end-to-end. A churn-focused regression alternates
viewer/host/hardware and viewer/miner/liquidity samples so the cache proves it
ignores regressions even when peers rotate advertised roles between scrapes,
while the new `ExplorerReadPayoutStalled`/`ExplorerAdPayoutStalled` alerts warn
when any role stays flat for thirty minutes after reporting non-zero totals.

The Grafana templates under `monitoring/grafana/` now dedicate a "Bridge" row to
the newly instrumented counters. Panels plot five-minute deltas for
`bridge_reward_claims_total`, `bridge_reward_approvals_consumed_total`,
`bridge_settlement_results_total{result,reason}`, and
`bridge_dispute_outcomes_total{kind,outcome}`, mirroring the counters defined in
`bridges/src/lib.rs`. A second column tracks cross-chain liquidity flow via
`bridge_liquidity_locked_total{asset}`, `bridge_liquidity_unlocked_total{asset}`,
`bridge_liquidity_minted_total{asset}`, and
`bridge_liquidity_burned_total{asset}` so operators can correlate reward spikes
with asset-specific inflows and outflows. The row now closes with four
remediation panels: `sum by (action, playbook)(increase(bridge_remediation_action_total[5m]))`
continues to display the recommended playbook, a companion panel charts
`sum by (action, playbook, target, status)(increase(bridge_remediation_dispatch_total[5m]))`
so dispatch successes, skips, and failures by target surface directly on the
dashboard, the acknowledgement panel tracks
`sum by (action, playbook, target, state)(increase(bridge_remediation_dispatch_ack_total[5m]))`
so downstream paging/governance hooks prove they have acknowledged or closed the
playbook, and a new histogram overlay renders
`histogram_quantile(0.50|0.95, sum by (le, playbook, state)(rate(bridge_remediation_ack_latency_seconds_bucket[5m])))`
alongside the policy gauge `bridge_remediation_ack_target_seconds{playbook,policy}`
so operators can monitor acknowledgement latency per playbook before escalation
windows expire and see the configured retry/escalation thresholds directly on
the chart. Operators can filter every legend to drill into a specific asset,
playbook, target, status, acknowledgement state, latency bucket, or policy, and
the same queries back the HTML snapshot so FIRST_PARTY_ONLY deployments never
rely on external Grafana instances to monitor bridge health. A dedicated
regression, `dashboards_include_bridge_counter_panels`, now parses each
generated Grafana JSON (dashboard/operator/telemetry/dev) to ensure the
reward-claim, approval, settlement, and dispute panels retain their queries and
legends across templates. A companion test, `dashboards_include_bridge_remediation_legends_and_tooltips`, locks the remediation row legends and descriptions so Grafana tooltips stay aligned with the PromQL on every generated template.

Pending follow-ups are now automated by policy instead of manual sweeps. Each
remediation action records `dispatch_attempts`, `auto_retry_count`, retry
timestamps, and cumulative follow-up notes so the aggregator can queue
first-party retry payloads once the acknowledgement window expires and escalate
automatically when the governance threshold trips. The acknowledgement parser
accepts plain-text hook responses (`"ack pager"`, `"closed: resolved"`, etc.) in
addition to JSON objects, promoting them to structured
`BridgeDispatchAckRecord`s that feed the dashboard panels and persisted action
state. New bridge alerts—`BridgeRemediationAckPending`,
`BridgeRemediationClosureMissing`, and `BridgeRemediationAckLatencyHigh`—fan out
from the same persisted metrics to warn when acknowledgements stall, closures
never arrive, or the observed p95 exceeds the configured policy target, keeping
paging/escalation coverage entirely first party.

Policy windows now vary per playbook. `TB_REMEDIATION_ACK_RETRY_SECS`,
`TB_REMEDIATION_ACK_ESCALATE_SECS`, and `TB_REMEDIATION_ACK_MAX_RETRIES` continue
to seed the defaults, while suffix overrides like
`TB_REMEDIATION_ACK_RETRY_SECS_GOVERNANCE_ESCALATION` (and the matching
`_ESCALATE_SECS`/`_MAX_RETRIES`) let operators tighten or relax the retry and
escalation thresholds for sensitive hooks without recompiling. Completion
latency feeds the new `bridge_remediation_ack_latency_seconds{playbook,state}`
histogram and persists the latest samples across restarts, so the Grafana row
and HTML snapshot plot p50/p95 acknowledgement times alongside the dispatch
counters even after an aggregator reboot. The CLI gained a first-party view as
well: `contract remediation bridge --aggregator http://agg:9000` prints the most
recent actions with retry history, follow-up notes, acknowledgement metadata,
and the matching dispatch log so operators can triage a backlog directly from
the aggregator without invoking external tooling. Operators can filter the
output with `--playbook`, `--peer`, and request machine-readable JSON via
`--json` to script downstream automations without leaving the first-party
binary.

The metrics aggregator now watches those counters for anomalous spikes. A
rolling detector maintains a 24-sample baseline per peer/metric/label set and
raises events when a new delta exceeds the historical mean by four standard
deviations (bounded by a minimum delta). Triggered events increment the
`bridge_anomaly_total` counter and flow to the `/anomalies/bridge` JSON endpoint,
which returns the offending metric, peer, labels, delta, and baseline stats.
Dashboards include a companion panel charting `increase(bridge_anomaly_total[5m])`
so operators can correlate alerts with reward claims, settlement submissions,
and dispute outcomes without leaving the first-party stack. The same detector
now publishes per-peer observations via
`bridge_metric_delta{metric,peer,labels}` and
`bridge_metric_rate_per_second{metric,peer,labels}` gauges, exposing the raw
counter deltas and normalised per-second growth so dashboards and alerting rules
can visualise bridge load across relayers in real time.

The Prometheus alert catalogue now ships with first-party validation across the
bridge, chain-health, dependency-registry, and treasury groups. The
`monitoring/src/bin/bridge-alert-validator` binary delegates to the new
`alert_validator` module, normalising every expression in
`monitoring/alert.rules.yml` and replaying canned datasets so bridge, consensus,
registry, and treasury alerts cannot change without matching first-party
coverage. CI invokes the validator after `cargo test --manifest-path
monitoring/Cargo.toml`, keeping the entire rule deck hermetic without relying on
promtool.

The shared dataset now includes recovery curves and partial-window samples for
both delta and rate series—covering global counters, asset-labelled data, and
dispute outcome slices. Additional fixtures exercise the
`result="failed",reason="quorum"` approvals flow plus
`kind="challenge",outcome="penalized"` dispute paths so the
`BridgeCounter*Skew` alerts stay quiet while an anomaly recovers or when fewer
than six samples are available, locking in the heuristics across future rule
edits.

The bridge alert group continues to consume the per-relayer gauges to raise
early warnings when a relayer’s growth deviates from historical norms.
`BridgeCounterDeltaSkew` and `BridgeCounterRateSkew` compare the latest sample to
a 30-minute average, require at least six observations, and trip whenever the
current delta/rate is three times the rolling mean while clearing a small
absolute floor (10 events per scrape or 0.5 events/sec). Alerts are emitted with
the metric/peer/label set in the annotations so operators can pivot straight
into the dashboards’ legends. Label-aware companions
`BridgeCounterDeltaLabelSkew` and `BridgeCounterRateLabelSkew` scope the same
logic to non-empty `labels!=""` selectors so asset- or failure-specific spikes
page without waiting for the aggregate counter to drift. The metrics aggregator
now persists each gauge’s last value, timestamp, and baseline window into the
in-house metrics store, so a restart resumes the prior state instead of treating
the first post-restart observation as a spike. When labelled anomalies fire, the
aggregator’s remediation engine evaluates the delta severity and emits
structured actions (page, throttle, quarantine, escalate) via the
`/remediation/bridge` JSON endpoint and the
`bridge_remediation_action_total{action,playbook}` counter. Each entry now
records the follow-up playbook (`incentive-throttle` or
`governance-escalation`) alongside the action, tracks
`acknowledged_at`/`closed_out_at` timestamps when downstream hooks confirm
receipt, and captures any `acknowledgement_notes` the pager/escalation system
returns. The response also carries a human-readable `annotation`, a curated
`dashboard_panels` list, and a `response_sequence` so runbooks can automate the
exact steps without relying on third-party tooling.

Remediation actions no longer stop at dashboards. The aggregator fans out each
playbook to first-party paging and governance hooks defined through
environment variables. Set `TB_REMEDIATION_PAGE_URLS`,
`TB_REMEDIATION_THROTTLE_URLS`, `TB_REMEDIATION_QUARANTINE_URLS`, or
`TB_REMEDIATION_ESCALATE_URLS` to comma-separated HTTPS destinations to receive
JSON `POST` payloads (`bridge-node` peer id, metric, labels, playbook, and a
`dispatched_at` timestamp). When automation prefers local queueing, the
matching `*_DIRS` variables instruct the aggregator to persist the same payload
under deterministic filenames inside a spool directory—other first-party
workers tail the directory and execute the playbooks without external tooling.
Every attempt increments `bridge_remediation_dispatch_total{action,playbook,target,status}`
and, when the hook returns acknowledgement metadata, the companion
`bridge_remediation_dispatch_ack_total{action,playbook,target,state}` counter. The
dispatch record appended to `/remediation/bridge/dispatches` includes the
acknowledgement payload (state, timestamp, notes) so operators can audit
`acknowledged`, `closed`, `pending`, or `invalid` replies alongside
`success`, `request_build_failed`, `payload_encode_failed`, `request_failed`,
`status_failed`, `persist_failed`, `join_failed`, or `skipped` outcomes by target
(`http`, `spool`, or `none`). A dedicated `bridge_remediation_spool_artifacts`
gauge tracks how many payloads remain queued on disk, and the bridge dashboard
row renders a matching panel so responders see outstanding spools without
scraping the filesystem. The gauges and the dispatch log let operators verify
that paging hooks, spool directories, and governance escalations are
acknowledged and closed out without leaving the first-party stack. Both dispatch
paths are logged at `INFO`, include the peer/metric/action trio, and retry on the
next anomaly if an endpoint is unavailable.

Spool artefacts persist across acknowledgement retries so failed hooks can be
replayed after a restart; once an action is acknowledged or closed the
aggregator drains the artefacts automatically, updates the
`bridge_remediation_spool_artifacts` gauge, and the restart regression asserts
the spool directory is clean before proceeding. Regression tests now seed a
per-case `RemediationSpoolSandbox` that fabricates isolated directories via
`sys::tempfile`, enables page/throttle/quarantine/escalate directories, and uses
`remediation_spool_sandbox_restores_environment` to confirm `TB_REMEDIATION_*_DIRS`
guards unwind after teardown—retry-heavy suites stop polluting `/tmp` while
staying entirely first party.
The contract remediation CLI mirrors the behaviour by emitting each action’s
`spool_artifacts` array in JSON
mode (with `--playbook`/`--peer` filters) so on-call responders can audit the
remaining payloads without touching third-party tooling.

Dependency policy status now lives in the same generated dashboard row. Panels
plot `dependency_registry_check_status{status}` gauges, drift counters, and the
age of the latest snapshot so operations can verify registry health without
leaving the first-party stack. The Prometheus alert group `dependency_registry`
pages when drift reappears or snapshots go stale, mirroring the telemetry stored
alongside release provenance.

### Snapshot CLI

Operators that prefer to run the snapshotter manually can invoke it directly:

```bash
(cd monitoring && cargo run --bin snapshot -- $TB_MONITORING_ENDPOINT)
```

The binary parses the same `TB_MONITORING_*` environment variables as the
native script, emitting `monitoring/output/index.html` alongside the metrics
specification. Exit codes reflect network status so automation can alert when a
fetch fails or the endpoint returns a non-success HTTP status.

The CLI installs a first-party `MonitoringRecorder` before scraping telemetry.
It increments `monitoring_snapshot_success_total` on clean runs and
`monitoring_snapshot_error_total` when a fetch fails, exposing structured
counters through the `foundation_metrics` facade so automation can poll recorder
state instead of parsing stderr.

Remote signer integrations emit `remote_signer_request_total`,
`remote_signer_success_total`, and `remote_signer_error_total{reason}` under the
telemetry feature flag, allowing dashboards to correlate multisig activity with
wallet QoS events. Pair these with the wallet-side `fee_floor_warning_total` and
`fee_floor_override_total` counters to spot signer outages that cause operators
to force submissions below the governance floor. The RPC client’s sanitized
`TB_RPC_FAULT_RATE` parsing ensures that chaos experiments never panic in
`gen_bool`; injected faults now surface as explicit
`RpcClientError::InjectedFault` log entries instead of crashing the dashboard
scrape loop.

### Cluster-wide peer metrics

Nodes can push their per-peer statistics to an external
`metrics-aggregator` service for fleet-level visibility.

#### Configuration

Set the `metrics_aggregator` section in `config.toml` with the aggregator `url`
and shared `auth_token`. Additional environment variables tune persistence:

- `AGGREGATOR_DB` — path to the first-party sled database directory (default:
  `./peer_metrics.db`).
- `AGGREGATOR_RETENTION_SECS` — prune entries older than this many seconds
  (default: `604800` for 7 days). The same value can be set in
  `metrics_aggregator.retention_secs` within `config.toml`.

Enable TLS by supplying `--tls-cert` and `--tls-key` files when starting the
aggregator. Nodes verify the certificate via the standard Rustls store.
Token-based auth uses the `auth_token`; when the token is stored on disk
both the node and aggregator reload it for new requests without requiring
a restart.

Snapshots persist across restarts in a disk-backed first-party sled store keyed by
peer ID. On startup the aggregator drops entries older than
`retention_secs` and schedules a periodic cleanup that prunes stale rows,
incrementing the `aggregator_retention_pruned_total` counter. Operators
can force a sweep by running `aggregator prune --before <timestamp>`.
`scripts/aggregator_backup.sh` and `scripts/aggregator_restore.sh` offer
simple archive and restore helpers for the database directory.

#### Behaviour and resilience

If the aggregator restarts or becomes unreachable, nodes queue updates
in memory and retry with backoff until the service recovers. The ingestion
pipeline now runs entirely on the first-party `httpd` router, matching the
node, gateway, and tooling stacks while reusing the runtime request builder.
Aggregated snapshots deduplicate on peer ID so multiple nodes reporting the
same peer collapse into a single record. The remaining roadmap item is to
swap the bespoke node RPC parser for `httpd::Router` so every surface shares
the same configuration knobs.

### High-availability deployment

Run multiple aggregators for resilience. Each instance now coordinates
through the bundled `InhouseEngine` lease table: the process that acquires
the `coordination/leader` row within its metrics database keeps the
leadership fence token alive, while followers tail the write-ahead log to
stay consistent. Operators can override the generated instance identifier
with `AGGREGATOR_INSTANCE_ID` or tune the lease timers via
`AGGREGATOR_LEASE_TTL_SECS`, `AGGREGATOR_LEASE_RENEW_MARGIN_SECS`, and
`AGGREGATOR_LEASE_RETRY_MS`. Nodes can discover aggregators through DNS SRV
records and automatically fail over when the leader becomes unreachable.
Load balancers should scrape `/healthz` on each instance and watch the
`aggregator_replication_lag_seconds` gauge for replica drift.

#### Metrics and alerts

The aggregator now emits metrics through the first-party `runtime::telemetry`
registry, rendering a Prometheus-compatible text payload without linking the
third-party `prometheus` crate. Gauges such as `cluster_peer_active_total` and
counters like `aggregator_ingest_total`, `aggregator_retention_pruned_total`,
and `bulk_export_total` are registered inside the in-house registry and served
at `/metrics`. The service installs the shared
`AggregatorRecorder` during startup so every `foundation_metrics` macro emitted
by runtime backends, TLS sinks, or tooling bridges back into those Prometheus
handles while preserving integer TLS fingerprint gauges and the runtime
spawn-latency histogram/pending-task gauge. The shared `http_env` sink feeds both the
`tls_env_warning_total{prefix,code}` counter and the
`tls_env_warning_last_seen_seconds{prefix,code}` gauge, and now publishes BLAKE3
fingerprint gauges/counters (`tls_env_warning_detail_fingerprint{prefix,code}`,
`tls_env_warning_variables_fingerprint{prefix,code}`,
`tls_env_warning_detail_fingerprint_total{prefix,code,fingerprint}`,
`tls_env_warning_variables_fingerprint_total{prefix,code,fingerprint}`) so
diagnostics fan-out and peer ingests keep dashboards up to date while restarts
rehydrate warning freshness and hashed payload counts from node-exported
gauges. Those fingerprint gauges are emitted as integer samples, preserving the
full 64-bit digest instead of rounding through IEEE754 conversions. The
`tls_env_warning_detail_fingerprint_total{prefix,code,fingerprint}`,
`tls_env_warning_variables_fingerprint_total{prefix,code,fingerprint}`) so
diagnostics fan-out and peer ingests keep dashboards up to date while restarts
rehydrate warning freshness and hashed payload counts from node-exported
gauges. The `tls_env_warning_detail_unique_fingerprints{prefix,code}` and
`tls_env_warning_variables_unique_fingerprints{prefix,code}` gauges expose how
many distinct hashes have been seen per label set, and the aggregator emits an
`observed new tls env warning … fingerprint` info log the first time a
non-`none` fingerprint arrives so operators can flag previously unseen payloads
in incident timelines. Operators can also query
`GET /tls/warnings/latest` for a JSON document summarising the latest warning
per `{prefix,code}` pair, including the accumulated total, the most recent
delta, the originating peer (when the increment arrived via ingestion), any
structured diagnostics detail, the last-seen timestamp, captured variables, and
per-fingerprint counts. Warning snapshots default to a seven-day retention
window (overridable via `AGGREGATOR_TLS_WARNING_RETENTION_SECS`); older entries
are pruned automatically so `/tls/warnings/latest` mirrors the current
operational state instead of accumulating historical noise. Recommended scrape targets remain both the
aggregator and the node exporters. Alert when `cluster_peer_active_total` drops
unexpectedly, when ingestion/export counters stop increasing, or when the
`TlsEnvWarningBurst` alert fires for any service prefix. The end-to-end
telemetry tests spin up a real aggregator instance, emit diagnostics warnings,
post peer ingests, and assert that `/metrics` and `/tls/warnings/latest`
reflect both sources, preventing regressions across the sink fan-out and HTTP
ingestion paths.
`GET /tls/warnings/status` complements the latest snapshot endpoint by
returning a retention summary (`retention_seconds`, `active_snapshots`,
`stale_snapshots`, and the newest/oldest timestamps). Operators should wire this
into runbooks so widening the retention window or clearing obsolete prefixes can
be verified without scraping metrics. The same data lands in Prometheus as
`tls_env_warning_retention_seconds`, `tls_env_warning_active_snapshots`,
`tls_env_warning_stale_snapshots`,
`tls_env_warning_most_recent_last_seen_seconds`, and
`tls_env_warning_least_recent_last_seen_seconds`, enabling alert rules without
extra templating. Grafana dashboards now include a "TLS env warnings (age
seconds)" panel sourced from
`clamp_min(time() - max by (prefix, code)(tls_env_warning_last_seen_seconds), 0)`
to visualise how long each warning has been quiet and to back alerts that guard
against forgotten TLS overrides. The TLS row also charts the hashed detail and
variables fingerprints (`tls_env_warning_*_fingerprint`), the unique hash counts
(`tls_env_warning_*_unique_fingerprints`), and per-fingerprint 5-minute deltas
derived from `increase(tls_env_warning_*_fingerprint_total[5m])` so engineers can
spot new hashes or sustained bursts at a glance. The new
`TlsEnvWarningNewDetailFingerprint`/`TlsEnvWarningNewVariablesFingerprint`
alerts fire when the unique gauges climb, while the
`TlsEnvWarningDetailFingerprintFlood` and
`TlsEnvWarningVariablesFingerprintFlood` alerts promote prolonged non-`none`
fingerprint surges to a paging signal. The `TlsEnvWarningSnapshotsStale` alert
continues to trigger when the stale gauge stays above zero for 15 minutes, and
operators can double-check the report with `contract tls status --aggregator
http://localhost:9000 --latest` (or `--json` for automation) to print suggested
remediation steps alongside the raw status payload.
For on-host validation, `contract telemetry tls-warnings` mirrors the same
data, exposes per-fingerprint counts, and now accepts `--probe-detail` /
`--probe-variables` flags to compute expected hashes locally before comparing
them with Prometheus output. The text view now includes an `ORIGIN` column that
matches the Prometheus `tls_env_warning_events_total{prefix,code,origin}`
label, making it trivial to line up local snapshots with dashboard filters. The
`monitoring compare-tls-warnings` binary wraps
this workflow by reading `contract telemetry tls-warnings --json`, fetching
`/tls/warnings/latest`, and verifying the Prometheus
`tls_env_warning_*` counters/gauges; any drift prints a labeled mismatch and sets
the exit code for automation. `/export/all` support bundles now bundle
`tls_warnings/latest.json` and `tls_warnings/status.json` so offline incidents
retain hashed payloads, fingerprint tallies, and retention metadata alongside
peer metrics.

### Metrics-to-logs correlation

The aggregator ingests runtime telemetry labels that include `correlation_id` and caches the most recent values per metric. When a counter such as `quic_handshake_fail_total{peer="…"}` spikes, the service issues a REST query against the node's `/logs/search` endpoint, saves the matching payload under `$TB_LOG_DUMP_DIR`, and increments `log_correlation_fail_total` when no records are found. These outbound fetches now run through the shared `httpd::HttpClient`, giving the service the same timeout and backoff behaviour as the node’s JSON-RPC client without pulling in `reqwest`. Operators can retrieve cached mappings via `GET /correlations/<metric>` or the CLI:

```bash
contract logs correlate-metric --metric quic_handshake_fail_total \
    --aggregator http://localhost:9300 --rows 20 --max-correlations 5
```

The log indexer records ingest offsets in the first-party, sled-backed `log_index::LogStore`, batches inserts with lightweight JSON payloads, supports encryption key rotation with passphrase prompts, and exposes both REST (`/logs/search`) and WebSocket (`/logs/tail`) streaming APIs for dashboards. `scripts/log_indexer_load.sh` stress-tests one million log lines, while integration tests under `node/tests/log_api.rs` validate the filters end-to-end. Legacy SQLite databases are migrated automatically when the indexer is built with `--features sqlite-migration`; once imported, the default build path keeps the dependency surface purely first-party. Set the `passphrase` option when invoking `index` (either through the CLI or RPC) to encrypt message bodies at rest; supply the same passphrase via the query string when using `/logs/search` or `/logs/tail` to decrypt results on the fly. CLI defaults resolve the store path via `--db`, `TB_LOG_STORE_PATH`, and finally `TB_LOG_DB_PATH` so dashboards inherit the new environment order without breaking older deployments.

When the node runs without the `telemetry` feature the `diagnostics::tracing` macros still compile but no additional sinks are installed, so subsystems that normally emit structured spans fall back to plain stderr diagnostics. RPC log streaming, mempool admission, and QUIC handshake validation all degrade gracefully: warnings appear in the system journal, counters remain untouched, and the RPC surface continues to return JSON errors. Enable `--features telemetry` whenever runtime metrics and structured spans are required.

The CLI, aggregator, and wallet stacks now share the new `httpd::uri` helpers for URL parsing and query encoding. Until the full HTTP router lands these helpers intentionally reject unsupported schemes and surface `UriError::InvalidAuthority` rather than guessing behaviour, so operators may see 501 responses when integrations send exotic URLs. The stub keeps the dependency graph first-party while we flesh out end-to-end parsers.

#### Threat model

Attackers may attempt auth token reuse, replay submissions, or file-path
traversal via `AGGREGATOR_DB`. Restrict token scope, use TLS, and run the
service under a dedicated user with confined file permissions.

Peer metrics exports sanitize relative paths, reject symlinks, and lock files during assembly to avoid race conditions. Only `.json`, `.json.gz`, or `.tar.gz` extensions are honored, and suspicious requests are logged with rate limiting. Disable exports entirely by setting `peer_metrics_export = false` in `config/default.toml` on sensitive nodes.

#### Bulk exports

Operators can download all peer snapshots in one operation via the aggregator’s `GET /export/all` endpoint. The response is a ZIP archive where each entry is `<peer_id>.json`. The binary `net stats export --all --path bulk.zip --rpc http://aggregator:9300` streams this archive to disk. The service rejects requests when the peer count exceeds `max_export_peers` and increments the `bulk_export_total` counter for visibility.
For sensitive deployments the archive can be encrypted in transit by passing an
in-house X25519 recipient (prefix `tbx1`):

```
net stats export --all --path bulk.tbenc --recipient <RECIPIENT>
```

The CLI forwards the recipient to the aggregator which encrypts the ZIP stream
and sets the `application/tb-envelope` content type. Recipients can decrypt the
payload with `crypto_suite::encryption::envelope::decrypt_with_secret`.

Alternatively, operators can supply a shared password to wrap the archive using
the same first-party primitives:

```
net stats export --all --path bulk.tbenc --password <PASSPHRASE>
```

Password-based responses advertise the `application/tb-password-envelope`
content type and can be opened with
`crypto_suite::encryption::envelope::decrypt_with_password`.

Key rotations propagate through the same channel. After issuing `net rotate-key`,
nodes increment `key_rotation_total` and persist the event to
`state/peer_key_history.log` as well as the cluster-wide metrics aggregator.
Old keys remain valid for five minutes to allow fleet convergence.

#### Deployment

`deploy/metrics-aggregator.yaml` ships a Kubernetes manifest that mounts the
database path and injects secrets for TLS keys and auth tokens.

#### Quick start

1. Launch the aggregator:
   ```bash
   AGGREGATOR_DB=/var/lib/tb/aggregator.db \
   metrics-aggregator --auth-token $TOKEN
   ```
2. Point a node to it by setting `metrics_aggregator.url` and
   `metrics_aggregator.auth_token` in `config.toml`.
3. Verify ingestion by hitting `http://aggregator:9300/metrics` and
   looking for `aggregator_ingest_total`.

#### Chaos readiness endpoints

The aggregator now ingests signed chaos attestations produced by the simulation
harness or live drills. Submit one or more `ChaosAttestation` payloads via
`POST /chaos/attest`:

```bash
curl -X POST http://aggregator:9300/chaos/attest \
  -H "content-type: application/json" \
  -d @chaos_attestations.json
```

Payloads must include the module (`"overlay"`, `"storage"`, or `"compute"`),
readiness and SLA thresholds (0.0–1.0), breach counts, observation window
boundaries, and an Ed25519 signature over the normalized digest. Invalid
signatures, digests, or out-of-range readiness values are rejected with
`400` responses and diagnostic logs; successful submissions return `202` and
update the following telemetry:

- `chaos_readiness{module,scenario}` — latest readiness score per module.
- `chaos_sla_breach_total` — cumulative SLA breaches recorded across attested
  scenarios.

Operators can retrieve the current readiness view via `GET /chaos/status`. The
response is an array of `ChaosReadinessSnapshot` objects containing the latest
scenario per module, normalized readiness figures, and signer metadata. These
snapshots back the `/chaos/status` Grafana panel and power automation that gates
releases on WAN chaos rehearsals.

`sim/chaos_lab.rs` also preserves every artefact in `chaos/archive/`. Each run
emits a `manifest.json` containing the file name, byte length, and BLAKE3 digest
for the snapshot, diff, overlay readiness table, and provider failover report,
and `latest.json` points at the newest run. A deterministic `run_id.zip` bundle
captures the same files, letting operators archive or mirror the entire set.
Optional `--publish-dir`, `--publish-bucket`, and `--publish-prefix` flags copy
the manifests and bundle into long-lived directories or S3-compatible buckets
through the first-party `foundation_object_store` client so dashboards and
release automation ingest the same preserved artefacts.

`tools/xtask chaos` consumes these manifests via manual
`foundation_serialization::json::Value` decoding, prints publish targets
alongside readiness regressions, provider churn, and duplicate-site analytics,
and still fails closed when overlay readiness drops or failover drills do not
produce diffs. `scripts/release_provenance.sh` refuses to continue unless
`chaos/archive/latest.json` and the referenced manifest are present, and
`scripts/verify_release.sh` parses the manifest to ensure every archived file
exists and that the recorded bundle size matches the on-disk `run_id.zip` before
approving a release.

#### Troubleshooting

| Status/Log message | Meaning | Fix |
| --- | --- | --- |
| `401 unauthorized` | Bad `auth_token` | Rotate token on both node and service |
| `503 unavailable` | Aggregator down | Node will retry; check service logs |
| `log query failed` in logs | log store directory unavailable or corrupt | Validate `TB_LOG_DB_PATH`, rerun the indexer, or migrate from the legacy backup |

Operators can clone the dashboard JSON and add environment-specific panels—for
example, graphing `subsidy_bytes_total{type="storage"}` per account or plotting
`rent_escrow_burned_ct_total` over time to spot churn. Exported JSONs should be
checked into a separate ops repository so upgrades can diff metric coverage.

These subsidy gauges directly reflect the CT-only economic model: `subsidy_bytes_total{type="read"}` increments when gateways serve acknowledged bytes, `subsidy_bytes_total{type="storage"}` tracks newly admitted blob data, and `subsidy_cpu_ms_total` covers deterministic edge compute. Rent escrow health is captured by `rent_escrow_locked_ct_total` (currently held deposits), `rent_escrow_refunded_ct_total`, and `rent_escrow_burned_ct_total`. The `subsidy_auto_reduced_total` counter records automatic multiplier down‑tuning when realised inflation drifts above the target, while `kill_switch_trigger_total` increments whenever governance activates the emergency kill switch. Monitoring these counters alongside `inflation.params` outputs allows operators to verify that multipliers match governance expectations and that no residual legacy-ledger fields remain. For the full rationale behind these metrics and the retirement of the auxiliary reimbursement ledger, see [system_changes.md](system_changes.md#ct-subsidy-unification-2024).

New acknowledgement plumbing introduces `read_ack_processed_total{result}`—watch
for sustained `invalid_signature` or `invalid_privacy` growth to catch malformed
signatures, failing proofs (in observe mode), or channel back-pressure before
receipts starve subsidy splits. Pair the counter with explorer snapshots of
`read_sub_*_ct` and `ad_*_ct` block fields to confirm governance-configured
viewer/host/hardware/verifier/liquidity shares and advertising settlements
remain in sync with observed acknowledgement volume. The compute-market
dashboard ships a "Read Ack Outcomes (5m delta)" panel charting
`increase(read_ack_processed_total[5m])` by result so the
`result="invalid_privacy"` label is visible alongside the existing series.

When `read_ack_processed_total{result="invalid_signature"}` or
`{result="invalid_privacy"}` accelerates, pivot into the response loop documented in [governance.md](governance.md#read-acknowledgement-anomaly-response):

- Validate that the node’s acknowledgement worker is draining (`read_ack_worker=drain` logs) and the `result="ok"` series continues to advance.
- Correlate the spike with gateway domains via the explorer `/receipts/domain/:id` endpoint or the CLI payout breakdown to see which providers continue to earn CT despite invalid signatures.
- If the ratio stays elevated after gateway remediations, coordinate with governance to invoke the emergency subsidy reduction until `invalid_signature` trends back toward background noise.

Storage ingest and repair telemetry tags every operation with the active coder and compressor so fallback rollouts can be tracked explicitly. Dashboards should watch `storage_put_object_seconds{erasure=...,compression=...}`, `storage_put_chunk_seconds{...}`, and `storage_repair_failures_total{erasure=...,compression=...}` alongside the `storage_coding_operations_total` counters to spot regressions when the XOR/RLE fallbacks are engaged. The repair loop also surfaces `algorithm_limited` log entries that can be scraped into incident timelines.

Settlement persistence adds complementary gauges:

- `SETTLE_APPLIED_TOTAL` – increments whenever a CT accrual, refund, or SLA burn is recorded. Pair this with `compute_market.audit` to ensure every ledger mutation hits telemetry (legacy industrial counters remain for compatibility and stay zero in production).
- `SETTLE_FAILED_TOTAL{reason="spend|penalize|refund"}` – surfaces errors during ledger mutation (for example, insufficient balance when penalizing an SLA violation). Any sustained growth warrants investigation before balances drift.
- `SETTLE_MODE_CHANGE_TOTAL{state="dryrun|armed|real"}` – tracks activation transitions, enabling alerts when a node unexpectedly reverts to dry-run mode.
- `matches_total{dry_run,lane}` – confirms the lane-aware matcher continues to produce receipts. Alert if a lane’s matches drop to zero while bids pile up.
- `match_loop_latency_seconds{lane}` – latency histogram for each lane’s batch cycle. Rising p95 suggests fairness windows are expiring before matches land.
- `receipt_persist_fail_total` – persistence failures writing lane-tagged receipts into the RocksDB-backed `ReceiptStore`.
- `SLASHING_BURN_CT_TOTAL` and `COMPUTE_SLA_VIOLATIONS_TOTAL{provider}` – expose aggregate burn amounts and per-provider violation counts. Alert if a provider exceeds expected thresholds or if burns stop entirely when violations continue.
- `COMPUTE_SLA_PENDING_TOTAL`, `COMPUTE_SLA_NEXT_DEADLINE_TS`, and `COMPUTE_SLA_AUTOMATED_SLASH_TOTAL` – track queued SLA items, the next enforcement window, and automated slashes triggered by sweeps. Alert if pending records grow without matching automated slashes or if the next deadline approaches zero without resolution.
- `settle_audit_mismatch_total` – raised when automated audit checks detect a mismatch between the ledger and the anchored receipts, typically via `TB_SETTLE_AUDIT_INTERVAL_MS` or CI replay jobs.

Dashboards should correlate these counters with the RocksDB health metrics (disk latency, file descriptor usage) and with RPC responses from `compute_market.provider_balances` and `compute_market.recent_roots`. A sudden plateau in `SETTLE_APPLIED_TOTAL` combined with stale Merkle roots usually indicates a stuck anchoring pipeline.

Mobile gateways expose their own telemetry slice: track `mobile_cache_hit_total` versus
`mobile_cache_miss_total` to validate cache effectiveness, alert on spikes in
`mobile_cache_reject_total` (insertions exceeding configured payload or count limits),
and watch `mobile_cache_sweep_total`/`mobile_cache_sweep_window_seconds` for sweep
health. Pair the gauges `mobile_cache_entry_total`, `mobile_cache_entry_bytes`,
`mobile_cache_queue_total`, and `mobile_cache_queue_bytes` with CLI `mobile-cache
status` output to verify offline queues drain after reconnects. Use
`mobile_tx_queue_depth` to trigger pager alerts when queued transactions exceed the
expected range for the deployment.

Background light-client probes report their state via
`the_block_light_client_device_status{field,freshness}`. Alert when `charging` or
`wifi` labels stay at `0` for longer than the configured `stale_after` window or
when `battery` remains below the configured threshold; otherwise background sync and
log uploads will stall.

During incident response, correlate subsidy spikes with `gov_*` metrics and
`read_denied_total{reason}` to determine whether rewards reflect legitimate
traffic or a potential abuse vector. Historical Grafana snapshots are valuable
for auditors reconstructing economic conditions around an event.

## Docker setup

`monitoring/docker-compose.yml` provisions both services. Configuration files
live under `monitoring/prometheus.yml` and `monitoring/grafana/dashboard.json`.
The native script now uses the foundation dashboard generator directly rather
than downloading Prometheus and Grafana bundles.

## Validation

CI launches the stack and lints the dashboard whenever files under `monitoring/` change.
The workflow runs `npm ci --prefix monitoring && make -C monitoring lint` and uploads the lint log as an artifact.
Run the lint locally with:

First install the Node dev dependencies (requires Node 20+):

```bash
npm ci --prefix monitoring
make -C monitoring lint
```

The lint uses `npx jsonnet-lint` to validate `grafana/dashboard.json` and will
fail on unsupported panel types.

### Dashboard generation

`make -C monitoring lint` regenerates `metrics.json` and `grafana/dashboard.json`
from metric definitions in `node/src/telemetry.rs` via the scripts in
`monitoring/tools`. Removed metrics are kept in the schema with `"deprecated": true`
and omitted from the dashboard. Each runtime telemetry counter or gauge becomes
an HTML panel in the generated dashboard (Grafana templates remain for legacy
deployments). The auto-generated dashboard provides a starting point for
operators to further customize panels.

## Synthetic chain health checks

`scripts/synthetic.sh` runs a mine → gossip → tip cycle using the `probe` CLI and emits runtime telemetry metrics:

- `synthetic_convergence_seconds` – wall-clock time from mining start until tip is observed.
- `synthetic_success_total` – number of successful end-to-end runs.
- `synthetic_fail_total{step}` – failed step counters for `mine`, `gossip`, and `tip`.

Just targets:

```bash
just probe:mine
just probe:gossip
just probe:tip
```

## Governance metrics and webhooks

Governance paths emit:

- `gov_votes_total` – vote count by proposal.
- `gov_activation_total` – successful proposal activations.
- `gov_rollback_total` – rollbacks triggered by conflicting proposals.
- `gov_activation_delay_seconds` – histogram of activation latency.
- `gov_open_proposals` and `gov_quorum_required` gauges.

If `GOV_WEBHOOK_URL` is set, governance events are POSTed to the given URL with
JSON payloads `{event, proposal_id}`.

## Alerting

Legacy Prometheus rules under `monitoring/alert.rules.yml` watch for:

- Convergence lag (p95 over 30s for 10m, pages).
- Consumer fee p90 exceeding `ConsumerFeeComfortP90Microunits` (warns).
- Industrial deferral ratio above 30% over 10m (warns).
- `read_denied_total{reason="limit"}` rising faster than baseline (warns).
- Subsidy counter spikes via `subsidy_bytes_total`/`subsidy_cpu_ms_total` (warns).
- Sudden `rent_escrow_locked_ct_total` growth (warns).

`scripts/telemetry_sweep.sh` runs the synthetic check, queries the runtime exporter for headline numbers, and writes a timestamped `status/index.html` colored green/orange/red.

### RPC aids

Some subsidy figures are not metrics but can be sampled over JSON-RPC.
Operators typically add a cron job that logs the output of `inflation.params`
and `stake.role` for their bond address. Persisting these snapshots alongside
Telemetry data provides a full accounting trail when reconciling payouts or
investigating anomalous subsidy shifts.
