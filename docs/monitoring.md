# Monitoring

The default dashboard bundles Prometheus and Grafana to visualize subsystem metrics.

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
`read_denied_total{reason}` counters, `credit_issued_total{source,region}`
metrics, and average log size derived from the `log_size_bytes` histogram. The repository
omits screenshot assets to keep the tree lightweight; after running a monitor
command, open Grafana and import `monitoring/grafana/dashboard.json` to explore
the dashboard.

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

`scripts/telemetry_sweep.sh` runs the synthetic check, queries Prometheus for headline numbers, and writes a timestamped `status/index.html` colored green/orange/red.
