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

The native script streams the node’s `/metrics` endpoint, regenerates
`monitoring/output/index.html` every few seconds, and serves it on
<http://127.0.0.1:8088>. Set `TELEMETRY_ENDPOINT` to track a remote node or
adjust `REFRESH_INTERVAL`/`FOUNDATION_DASHBOARD_PORT` to suit local
requirements. `monitoring/prometheus.yml` now documents canonical telemetry
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
at `/metrics`. Recommended scrape targets remain both the aggregator and the
node exporters. Alert when `cluster_peer_active_total` drops unexpectedly or
when ingestion/export counters stop increasing.

### Metrics-to-logs correlation

The aggregator ingests runtime telemetry labels that include `correlation_id` and caches the most recent values per metric. When a counter such as `quic_handshake_fail_total{peer="…"}` spikes, the service issues a REST query against the node's `/logs/search` endpoint, saves the matching payload under `$TB_LOG_DUMP_DIR`, and increments `log_correlation_fail_total` when no records are found. These outbound fetches now run through the shared `httpd::HttpClient`, giving the service the same timeout and backoff behaviour as the node’s JSON-RPC client without pulling in `reqwest`. Operators can retrieve cached mappings via `GET /correlations/<metric>` or the CLI:

```bash
contract logs correlate-metric --metric quic_handshake_fail_total \
    --aggregator http://localhost:9300 --rows 20 --max-correlations 5
```

The log indexer records ingest offsets in the first-party log store, batches inserts with lightweight JSON payloads, supports encryption key rotation with passphrase prompts, and exposes both REST (`/logs/search`) and WebSocket (`/logs/tail`) streaming APIs for dashboards. `scripts/log_indexer_load.sh` stress-tests one million log lines, while integration tests under `node/tests/log_api.rs` validate the filters end-to-end. Legacy SQLite databases are migrated automatically when the indexer is built with `--features sqlite-migration`; once imported, the default build path keeps the dependency surface purely first-party. Set the `passphrase` option when invoking `index` (either through the CLI or RPC) to encrypt message bodies at rest; supply the same passphrase via the query string when using `/logs/search` or `/logs/tail` to decrypt results on the fly.

When the node runs without the `telemetry` feature the `tracing` crate is not linked, so subsystems that normally emit structured spans fall back to plain stderr diagnostics. RPC log streaming, mempool admission, and QUIC handshake validation all degrade gracefully: warnings appear in the system journal, counters remain untouched, and the RPC surface continues to return JSON errors. Enable `--features telemetry` whenever runtime metrics and structured spans are required.

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
