# Monitoring Dashboards

This directory contains subsystem-specific Grafana dashboards that complement
the primary telemetry bundle under `monitoring/grafana/`. Each JSON file is
ready to import once the Prometheus stack is running.

- `compute_market_dashboard.json` visualises backlog factors, fee-floor
  enforcement, courier retry behaviour, and the SLA violation counters alongside
  the rolling `fee_floor_current` gauge so operators can compare pricing policy
  with realised demand.

The consolidated cluster dashboard that ships with the repo lives at
`monitoring/grafana/telemetry.json`. It already exposes governance rollout
metrics (`release_quorum_fail_total`, `release_installs_total`), QUIC diagnostics
with per-peer retransmit and handshake panels, and log-correlation alerts fed by
the metrics aggregator.

Import any of these dashboards after running `make monitor` (or the native
equivalent) and ensure nodes start with `--metrics-addr` and
`--features telemetry`. Dashboards assume Prometheus on `localhost:9090`; edit the
embedded `datasource` block if your deployment differs.
