# Monitoring

The default dashboard bundles Prometheus and Grafana to visualize subsystem metrics.

## Quick start

```bash
make monitor
```

Prometheus scrapes the node at `host.docker.internal:9898` while Grafana serves a preloaded dashboard on <http://localhost:3000>.

Panels include mempool size, banned peers, and average log size derived from
the `log_size_bytes` histogram. The repository omits screenshot assets to keep
the tree lightweight; after running `make monitor`, open Grafana and import
`monitoring/grafana/dashboard.json` to explore the dashboard.

## Docker setup

`monitoring/docker-compose.yml` provisions both services. Configuration files live under `monitoring/prometheus.yml` and `monitoring/grafana/dashboard.json`.

## Validation

CI lints the dashboard JSON via `grafonnet` to catch schema errors.
