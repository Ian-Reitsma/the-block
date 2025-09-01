# Monitoring Dashboards

This directory contains Grafana dashboards for core subsystems.

- `compute_market_dashboard.json` visualizes backlog factors and courier metrics.
- `governance_dashboard.json` graphs proposal votes, rollbacks, and activation delays.
- `network_dashboard.json` tracks PoH ticks, turbine fanout, gossip convergence, `read_denied_total{reason}` counters, and `credit_issued_total{source}` metrics.

Import dashboards into Grafana after running `make monitor` and ensure the node is started with `--metrics-addr` and `--features telemetry`.
