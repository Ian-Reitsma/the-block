# Run a node

## Hardware
- 4 cores
- 8 GB RAM
- SSD with at least 50 GB free

## Ports
- P2P: `3033` (TCP and UDP if using QUIC)
- RPC: `3030`
- Metrics: `9898`

## QUIC mode

Start the node with `--quic` to enable QUIC gossip alongside TCP. The listener
binds to a UDP port specified by `--quic-port` (default `3033`). On first run a
self-signed certificate and key are written to `<data_dir>/quic.cert` and
`<data_dir>/quic.key` with `0600` permissions; subsequent restarts reuse these
files.  Certificates rotate automatically after the number of days specified by
`--quic-cert-ttl-days` (default 30). Ensure the key files remain owner-readable
only to avoid peers rejecting the endpoint.

## Quickstart
```sh
curl -LO <release-tar>
./scripts/verify_release.sh node-<ver>-x86_64.tar.gz checksums.txt checksums.txt.sig
mkdir -p ~/.block
 tar -xzf node-<ver>-x86_64.tar.gz -C ~/.block
~/.block/node --datadir ~/.block/datadir --config ~/.block/config.toml
```

Bond CT for a service role once the node is running:

```sh
cargo run --example wallet stake-role storage 100 --seed <hex>
```

## Inspecting peers

Use the networking CLI to inspect and manage per-peer metrics:

```sh
net stats <peer_id>
net stats reset <peer_id>
net stats export <peer_id> --path /tmp/peer.json
net stats --all
net stats reputation <peer_id>
```

`net stats` prints request, byte, and drop counters for the given peer. `net
stats reset` clears all counters, incrementing `peer_stats_reset_total{peer_id}`
and removing the peer from Prometheus until traffic resumes. `net stats export`
writes a JSON snapshot for offline analysis. `net stats --all` paginates through
all tracked peers, while `net stats reputation` shows the current reputation
score used for adaptive rate limits.

### systemd
Create `/etc/systemd/system/the-block.service`:
```ini
[Unit]
Description=The Block node
After=network.target

[Service]
ExecStart=/home/user/.block/node --datadir /home/user/.block/datadir --config /home/user/.block/config.toml
Restart=always
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```
Enable and start:
```sh
systemctl enable --now the-block
```

### Firewall
Allow P2P and metrics if required; restrict RPC to localhost.
Run the node with `--metrics-addr` and `--features telemetry` to surface
`read_denied_total` and `subsidy_bytes_total{type="storage"}` counters for monitoring.

## Difficulty monitoring

Query the current proof-of-work difficulty via JSON-RPC:

```bash
curl -s localhost:26658/consensus.difficulty
# {"difficulty":12345,"timestamp_millis":1700000000000}
```

Prometheus metrics expose retarget activity and clamp events:

```bash
curl -s localhost:9898/metrics | rg '^difficulty_'
```

The `difficulty_retarget_total` and `difficulty_clamp_total` counters should
advance roughly once per block under normal operation.

## Example workloads

Sample workload descriptors live under `examples/workloads/`. Run one with:

```bash
cargo run --example run_workload examples/workloads/cpu_only.json
```

Replace the path with `gpu_inference.json` or `multi_gpu.json` to request GPU
providers. The example verifies the JSON schema and prints the derived
capability requirements.
