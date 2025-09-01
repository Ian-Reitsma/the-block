# Monitoring Dashboards

This directory contains Grafana dashboards for core subsystems.

- `compute_market_dashboard.json` visualizes backlog factors and courier metrics.
- `governance_dashboard.json` graphs proposal votes, rollbacks, and activation delays.
- `network_dashboard.json` tracks PoH ticks, turbine fanout, gossip convergence, `read_denied_total{reason}` counters, and `credit_issued_total{source}` metrics.
- `settlement_dashboard.json` surfaces receipt indexing lag and `settle_audit_mismatch_total` from the CI audit job.
- `storage_dashboard.json` records disk-full events via `storage_disk_full_total` and recovery durations.

Import dashboards into Grafana after running `make monitor` and ensure the node is started with `--metrics-addr` and `--features telemetry`.
Dashboards expect Prometheus running on `localhost:9090`; edit the `datasource` section if your topology differs.
