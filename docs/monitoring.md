# Monitoring

The default dashboard bundles Prometheus and Grafana to visualize subsystem metrics.
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

The native script verifies SHA256 checksums for the downloaded Prometheus and
Grafana archives before extracting them. Add `DETACH=1` to run it without
blocking the calling shell.

Prometheus scrapes the node at `host.docker.internal:9898` while Grafana serves a preloaded dashboard on <http://localhost:3000>.

Panels include per-lane mempool size, banned peers, gossip duplicate counts,
`read_denied_total{reason}` counters, subsidy gauges (`subsidy_bytes_total{type}`, `subsidy_cpu_ms_total`, `rent_escrow_locked_ct_total`),
per-peer request/drop panels from `peer_request_total`/`peer_drop_total{reason}`,
scheduler match histograms (`scheduler_match_total{result}`,
`scheduler_provider_reputation`) and average log size derived from the
`log_size_bytes` histogram. Fee-floor charts combine `fee_floor_current` with
`fee_floor_warning_total{lane}`/`fee_floor_override_total{lane}` so operators can
trace wallet guidance, and DID anchor panels plot `did_anchor_total` alongside
recent `/dids` history for cross-navigation. Additional gauges expose
`subsidy_auto_reduced_total` and `kill_switch_trigger_total` so operators can
correlate reward shifts with governance interventions. After running a monitor
command, open Grafana and import `monitoring/grafana/dashboard.json` to explore
the live panels.

### Cluster-wide peer metrics

Nodes can push their per-peer statistics to an external
`metrics-aggregator` service for fleet-level visibility.

#### Configuration

Set the `metrics_aggregator` section in `config.toml` with the aggregator `url`
and shared `auth_token`. Additional environment variables tune persistence:

- `AGGREGATOR_DB` — path to the sled database directory (default:
  `./peer_metrics.db`).
- `AGGREGATOR_RETENTION_SECS` — prune entries older than this many seconds
  (default: `604800` for 7 days). The same value can be set in
  `metrics_aggregator.retention_secs` within `config.toml`.

Enable TLS by supplying `--tls-cert` and `--tls-key` files when starting the
aggregator. Nodes verify the certificate via the standard Rustls store.
Token-based auth uses the `auth_token`; when the token is stored on disk
both the node and aggregator reload it for new requests without requiring
a restart.

Snapshots persist across restarts in a disk-backed sled store keyed by
peer ID. On startup the aggregator drops entries older than
`retention_secs` and schedules a periodic cleanup that prunes stale rows,
incrementing the `aggregator_retention_pruned_total` counter. Operators
can force a sweep by running `aggregator prune --before <timestamp>`.
`scripts/aggregator_backup.sh` and `scripts/aggregator_restore.sh` offer
simple archive and restore helpers for the database directory.

#### Behaviour and resilience

If the aggregator restarts or becomes unreachable, nodes queue updates
in memory and retry with backoff until the service recovers. Aggregated
snapshots deduplicate on peer ID so multiple nodes reporting the same
peer collapse into a single record.

### High-availability deployment

Run multiple aggregators for resilience. Each instance performs leader
election via an external key-value store such as `etcd`; followers tail a
write-ahead log to stay consistent. Nodes can discover aggregators through
DNS SRV records and automatically fail over when the leader becomes
unreachable. Load balancers should scrape `/healthz` on each instance and
watch the `aggregator_replication_lag_seconds` gauge for replica drift.

#### Metrics and alerts

The aggregator exposes Prometheus gauges `cluster_peer_active_total` and
counters `aggregator_ingest_total`. Recommended scrape targets are both
the aggregator itself and the node exporters. Alert when
`cluster_peer_active_total` drops unexpectedly or when
`aggregator_ingest_total` stops increasing.

### Metrics-to-logs correlation

The aggregator ingests Prometheus labels that include `correlation_id` and caches the most recent values per metric. When a counter such as `quic_handshake_fail_total{peer="…"}` spikes, the service issues a REST query against the node's `/logs/search` endpoint, saves the matching payload under `$TB_LOG_DUMP_DIR`, and increments `log_correlation_fail_total` when no records are found. Operators can retrieve cached mappings via `GET /correlations/<metric>` or the CLI:

```bash
contract logs correlate-metric --metric quic_handshake_fail_total \
    --aggregator http://localhost:9300 --rows 20 --max-correlations 5
```

The log indexer records ingest offsets in SQLite, batches inserts with prepared statements, supports encryption key rotation with passphrase prompts, and exposes both REST (`/logs/search`) and WebSocket (`/logs/tail`) streaming APIs for dashboards. `scripts/log_indexer_load.sh` stress-tests one million log lines, while integration tests under `node/tests/log_api.rs` validate the filters end-to-end. The node crate now depends on `rusqlite` (built with the bundled SQLite engine) at runtime so operators do not need a system SQLite installation. Set the `passphrase` option when invoking `index` (either through the CLI or RPC) to encrypt message bodies at rest; supply the same passphrase via the query string when using `/logs/search` or `/logs/tail` to decrypt results on the fly.

When the node runs without the `telemetry` feature the `tracing` crate is not linked, so subsystems that normally emit structured spans fall back to plain stderr diagnostics. RPC log streaming, mempool admission, and QUIC handshake validation all degrade gracefully: warnings appear in the system journal, counters remain untouched, and the RPC surface continues to return JSON errors. Enable `--features telemetry` whenever Prometheus metrics and structured spans are required.

#### Threat model

Attackers may attempt auth token reuse, replay submissions, or file-path
traversal via `AGGREGATOR_DB`. Restrict token scope, use TLS, and run the
service under a dedicated user with confined file permissions.

Peer metrics exports sanitize relative paths, reject symlinks, and lock files during assembly to avoid race conditions. Only `.json`, `.json.gz`, or `.tar.gz` extensions are honored, and suspicious requests are logged with rate limiting. Disable exports entirely by setting `peer_metrics_export = false` in `config/default.toml` on sensitive nodes.

#### Bulk exports

Operators can download all peer snapshots in one operation via the aggregator’s `GET /export/all` endpoint. The response is a ZIP archive where each entry is `<peer_id>.json`. The binary `net stats export --all --path bulk.zip --rpc http://aggregator:9300` streams this archive to disk. The service rejects requests when the peer count exceeds `max_export_peers` and increments the `bulk_export_total` counter for visibility.
For sensitive deployments the archive can be encrypted in transit by passing an `age` recipient:

```
net stats export --all --path bulk.zip.age --age-recipient <RECIPIENT>
```
The CLI forwards the recipient to the aggregator which encrypts the ZIP stream and sets the `application/age` content type.

Alternatively, operators can supply an OpenSSL passphrase to encrypt with AES-256-CBC:

```
net stats export --all --path bulk.zip.enc --openssl-pass <PASSPHRASE>
```
The first 16 bytes of the response contain the IV; the remainder is the ciphertext.

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
| `db_locked` in logs | SQLite busy | Place DB on faster disk or increase backoff |

Operators can clone the dashboard JSON and add environment-specific panels—for
example, graphing `subsidy_bytes_total{type="storage"}` per account or plotting
`rent_escrow_burned_ct_total` over time to spot churn. Exported JSONs should be
checked into a separate ops repository so upgrades can diff metric coverage.

These subsidy gauges directly reflect the CT-only economic model: `subsidy_bytes_total{type="read"}` increments when gateways serve acknowledged bytes, `subsidy_bytes_total{type="storage"}` tracks newly admitted blob data, and `subsidy_cpu_ms_total` covers deterministic edge compute. Rent escrow health is captured by `rent_escrow_locked_ct_total` (currently held deposits), `rent_escrow_refunded_ct_total`, and `rent_escrow_burned_ct_total`. The `subsidy_auto_reduced_total` counter records automatic multiplier down‑tuning when realised inflation drifts above the target, while `kill_switch_trigger_total` increments whenever governance activates the emergency kill switch. Monitoring these counters alongside `inflation.params` outputs allows operators to verify that multipliers match governance expectations and that no residual legacy-ledger fields remain. For the full rationale behind these metrics and the retirement of the third-token ledger, see [system_changes.md](system_changes.md#2024-third-token-ledger-removal-and-ct-subsidy-transition).

During incident response, correlate subsidy spikes with `gov_*` metrics and
`read_denied_total{reason}` to determine whether rewards reflect legitimate
traffic or a potential abuse vector. Historical Grafana snapshots are valuable
for auditors reconstructing economic conditions around an event.

## Docker setup

`monitoring/docker-compose.yml` provisions both services. Configuration files
live under `monitoring/prometheus.yml` and `monitoring/grafana/dashboard.json`.
The native script downloads Prometheus and Grafana into `monitoring/bin/` and
launches them with these same configs.

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
and omitted from the dashboard. Each Prometheus counter or gauge becomes a
Grafana timeseries panel. The auto-generated dashboard provides a starting point
for operators to further customize panels.

## Synthetic chain health checks

`scripts/synthetic.sh` runs a mine → gossip → tip cycle using the `probe` CLI and emits Prometheus metrics:

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

Prometheus rules under `monitoring/alert.rules.yml` watch for:

- Convergence lag (p95 over 30s for 10m, pages).
- Consumer fee p90 exceeding `ConsumerFeeComfortP90Microunits` (warns).
- Industrial deferral ratio above 30% over 10m (warns).
- `read_denied_total{reason="limit"}` rising faster than baseline (warns).
- Subsidy counter spikes via `subsidy_bytes_total`/`subsidy_cpu_ms_total` (warns).
- Sudden `rent_escrow_locked_ct_total` growth (warns).

`scripts/telemetry_sweep.sh` runs the synthetic check, queries Prometheus for headline numbers, and writes a timestamped `status/index.html` colored green/orange/red.

### RPC aids

Some subsidy figures are not metrics but can be sampled over JSON-RPC.
Operators typically add a cron job that logs the output of `inflation.params`
and `stake.role` for their bond address. Persisting these snapshots alongside
Prometheus data provides a full accounting trail when reconciling payouts or
investigating anomalous subsidy shifts.
