# Multi-Node LAN Runbook (1 PC + 2 Mac M1)

This guide walks through bringing up a 3-node LAN cluster (primary PC + two Macs), wiring first-party telemetry, and running the multi-node overlay smoke test. Commands are split by OS where it matters.

## Roles and Ports

| Role     | RPC        | Status     | Metrics    | QUIC/Gossip |
|----------|------------|------------|------------|-------------|
| primary  | 3030       | 3031       | 9898       | 9000        |
| replica1 | 4030       | 4031       | 9899       | 9001        |
| observer | 5030       | 5031       | 9900       | 9002        |

Adjust ports if they collide; keep the order consistent for the test env var.

## Prerequisites
- Rust toolchain installed.
- Repo checked out on all three machines.
- First-party only (no Prometheus/Grafana). Telemetry via `/metrics` + first-party dashboard and metrics-aggregator.
- Shared LAN with static IPs (examples: 192.168.1.10/11/12).
- Verify toolchain on each host: `cargo --version`.
- Install helpers: `jq` (JSON), `curl` (already present on macOS/Fedora).
- Optional but faster: prebuild binaries on each host:
  - `cargo build -p the_block --features telemetry,quic`
  - `cargo build -p metrics-aggregator`

## Firewall Rules

### Fedora (primary PC)
```bash
sudo firewall-cmd --permanent --add-port=9000-9010/tcp
sudo firewall-cmd --permanent --add-port=9000-9010/udp
sudo firewall-cmd --permanent --add-port=9898-9900/tcp   # metrics endpoints per node
sudo firewall-cmd --permanent --add-port=9000/tcp        # metrics-aggregator on observer (if you proxy)
sudo firewall-cmd --reload
# If SELinux blocks listener binds, allow the node binary:
# sudo semanage port -a -t http_port_t -p tcp 9000-9900
```

### macOS (both Macs)
System Settings → Network → Firewall → Options:
- Allow incoming for “the-block”.
- Allow incoming for telemetry port (metrics) and metrics-aggregator (observer Mac).

## Bring Up the Cluster

> Run these on each machine from the repo root. Scripts live under `scripts/multi-node/`.

### 0) Prepare data roots (optional)
```bash
mkdir -p $HOME/.the_block/multi-node
```

### 1) Start the metrics aggregator (observer Mac)
```bash
ADDR=0.0.0.0:9000 \
DATA_ROOT=$HOME/.the_block/multi-node \
./scripts/multi-node/run-aggregator.sh
```
- Keep this terminal open (logs stream to stdout). Default token: `local-dev-token` (override via `TOKEN=...`).

### 2) Start each node

Each command enables QUIC + telemetry, sets distinct RPC/metrics/status/QUIC ports, and uses range-boost discovery to find peers on LAN.

**Primary (Fedora PC)**
```bash
ROLE=primary \
DATA_ROOT=$HOME/.the_block/multi-node \
./scripts/multi-node/run-node.sh
```

**Replica #1 (Mac #1)**
```bash
ROLE=replica1 \
DATA_ROOT=$HOME/.the_block/multi-node \
./scripts/multi-node/run-node.sh
```

**Observer (Mac #2)**
```bash
ROLE=observer \
DATA_ROOT=$HOME/.the_block/multi-node \
./scripts/multi-node/run-node.sh
```

Notes:
- Override addresses if you need to bind to a specific interface, e.g. `RPC_ADDR=192.168.1.11:4030`.
- Range-boost uses `TB_MESH_STATIC_PEERS` if you want to hardcode peers: `TB_MESH_STATIC_PEERS=192.168.1.10:9000,192.168.1.11:9001,192.168.1.12:9002`.
- Increase logging detail when diagnosing mesh connectivity:
```bash
RUST_LOG=debug ROLE=primary ./scripts/multi-node/run-node.sh
```

## Verify Connectivity

From any host with access to the RPC ports:
```bash
curl -s -X POST http://192.168.1.10:3030/ \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"net.overlay_status","params":[]}'
```
Check that `active_peers` equals 2 on each node (with three nodes total).

Check peer stats once peers appear (replace PEER_ID with one from overlay_status):
```bash
curl -s -X POST http://192.168.1.10:3030/ \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"net.peer_stats","params":{"peer_id":"<PEER_ID>"}}'
```

QUIC certificate inspection (primary):
```bash
curl -s -X POST http://192.168.1.10:3030/ \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"net.quic_certs","params":[]}'
```

## Telemetry (first-party only)

- Metrics endpoint per node: `http://<node-ip>:<metrics-port>/metrics`
- Render dashboard locally:
```bash
python monitoring/tools/render_foundation_dashboard.py http://192.168.1.10:9898/metrics
open monitoring/output/index.html   # macOS
# or xdg-open on Linux
```
- Aggregator (observer) lives at `http://<observer-ip>:9000` with the token you set (default `local-dev-token`).
- Quick check against aggregator:
```bash
curl -s -H "authorization: Bearer local-dev-token" http://192.168.1.12:9000/wrappers | head
```

## Run the Multi-Node Smoke Test

Set the RPC endpoints env var (comma-separated) and run the test harness (can be done from any machine with repo + cargo; it calls the live cluster):
```bash
export TB_MULTI_NODE_RPC=192.168.1.10:3030,192.168.1.11:4030,192.168.1.12:5030
./scripts/multi-node/run-cluster-tests.sh
```
The test polls `net.overlay_status` on each node and waits for peer convergence.

## Troubleshooting
- **active_peers < 2**: ensure ports are open, IPs correct, and QUIC/TCP reachable. Set `TB_MESH_STATIC_PEERS` to force discovery.
- **RPC timeout**: widen `TB_RPC_TIMEOUT_MS` (default 5000) or ensure `TB_RPC_TOKENS_PER_SEC` isn’t set too low.
- **Telemetry empty**: verify `--metrics-addr` was passed (scripts set it) and that `telemetry` feature was built (scripts set `FEATURES=telemetry,quic`).
- **SELinux denials (Fedora)**: check `sudo journalctl -t setroubleshoot` and allow ports via `semanage port -a -t http_port_t -p tcp 9000-9900` if blocked.
- **Firewall still blocking**: confirm rules are loaded (`sudo firewall-cmd --list-ports`) and that you bound to the correct interface (use `RPC_ADDR=0.0.0.0:3030` if needed).
- **Peer discovery slow**: set `TB_MESH_STATIC_PEERS` with the three QUIC ports, or restart nodes after peers are reachable.

## Tear Down
Stop processes with `Ctrl+C` in each terminal. Data lives under `~/.the_block/multi-node/<role>`; delete if you want a clean slate.
