# Monitoring

The default dashboard bundles Prometheus and Grafana to visualize subsystem metrics.

## Quick start

Docker (default):

```bash
make monitor
```

Native (no Docker or `--native-monitor`):

```bash
scripts/monitor_native.sh
```

`make monitor --native-monitor` and `./bootstrap.sh --native-monitor` call the
same script. When Docker isn't installed or the daemon is stopped, these
commands automatically fall back to the native binaries.

The native script verifies SHA256 checksums for the downloaded Prometheus and
Grafana archives before extracting them.

Prometheus scrapes the node at `host.docker.internal:9898` while Grafana serves a preloaded dashboard on <http://localhost:3000>.

Panels include mempool size, banned peers, and average log size derived from
the `log_size_bytes` histogram. The repository omits screenshot assets to keep
the tree lightweight; after running a monitor command, open Grafana and import
`monitoring/grafana/dashboard.json` to explore the dashboard.

## Docker setup

`monitoring/docker-compose.yml` provisions both services. Configuration files
live under `monitoring/prometheus.yml` and `monitoring/grafana/dashboard.json`.
The native script downloads Prometheus and Grafana into `monitoring/bin/` and
launches them with these same configs.

## Validation

CI briefly launches the stack and then lints the dashboard JSON. Run the lint
locally with:

First install the Node dev dependencies (requires Node 20+):

```bash
npm ci
make -C monitoring lint
```

The lint uses `npx jsonnet-lint` to validate `grafana/dashboard.json` and will
fail on unsupported panel types.
