# Monitoring Dashboards

This directory contains subsystem-specific Grafana dashboards that complement
the primary telemetry bundle under `monitoring/grafana/`. Each JSON file is
ready to import once the Prometheus stack is running.

- `compute_market_dashboard.json` visualises backlog factors, fee-floor
  enforcement, courier retry behaviour, and the SLA violation counters alongside
  the rolling `fee_floor_current` gauge and the companion
  `fee_floor_warning_total{lane}`/`fee_floor_override_total{lane}` counters so
  operators can compare pricing policy with realised demand. Governance changes
  to `mempool.fee_floor_window` and `mempool.fee_floor_percentile` increment
  `fee_floor_window_changed_total` and surface in the same dashboard.
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
`--features telemetry`. Dashboards assume Prometheus on `localhost:9090`; edit the
embedded `datasource` block if your deployment differs.
