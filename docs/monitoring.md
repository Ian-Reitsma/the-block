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
`read_denied_total{reason}` counters, subsidy gauges (`subsidy_bytes_total{type}`, `subsidy_cpu_ms_total`, `rent_escrow_locked_ct_total`),
per-peer request/drop panels from `peer_request_total`/`peer_drop_total{reason}`,
scheduler match histograms (`scheduler_match_total{result}`,
`scheduler_provider_reputation`) and average log size derived from the
`log_size_bytes` histogram. Additional gauges expose `subsidy_auto_reduced_total`
and `kill_switch_trigger_total` so operators can correlate reward shifts with
governance interventions. The repository omits screenshot assets to keep the
tree lightweight; after running a monitor command, open Grafana and import
`monitoring/grafana/dashboard.json` to explore the dashboard.

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
