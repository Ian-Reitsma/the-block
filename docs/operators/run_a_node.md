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

## Config reload

The node watches `config/default.toml` for changes and reloads rate-limit and reputation settings without restart. Trigger a manual reload with `config reload` from the CLI. Successful reloads increment `config_reload_total{result="ok"}` and update `config_reload_last_ts`; malformed files are logged and ignored.

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
net stats --backpressure
```

`net stats` prints request, byte, and drop counters for the given peer. `net
stats reset` clears all counters, incrementing `peer_stats_reset_total{peer_id}`
and removing the peer from Prometheus until traffic resumes. `net stats export`
writes a JSON snapshot for offline analysis. `net stats --all` paginates through
all tracked peers, while `net stats reputation` shows the current reputation
score used for adaptive rate limits.

| Flag | Description |
| ---- | ----------- |
| `--format table\|json` | Choose human-readable tables or machine-friendly JSON. |
| `--drop-reason <reason>` | Filter peers by a specific drop cause such as `rate_limit`. |
| `--min-reputation <score>` | Only include peers with reputation at or above the given value. |
| `--offset <n>` / `--limit <m>` | Page through large peer sets; combine with `--all` to stream every peer. |
| `--backpressure` | Show peers currently throttled for exceeding quotas. |

Rows with a drop rate ≥5 % render in yellow and ≥20 % in red to flag misbehaving
peers. Exit codes convey status: `0` on success, `2` if a peer is unknown, and
`3` when the RPC server rejects the request. Examples:

```bash
# JSON output for high-reputation peers dropping messages due to rate limits
net stats --format json --drop-reason rate_limit --min-reputation 0.8

# Interactive pagination of all peers (press Enter to advance pages)
net stats --all --limit 50
```

The CLI honours `peer_metrics_export` and `max_peer_metrics` configuration
limits. See [docs/gossip.md](../gossip.md) for protocol details and additional
RPC examples.

Backpressure limits derive from `p2p_max_per_sec` and `p2p_max_bytes_per_sec`
in `config.toml`; throttle duration defaults to `TB_THROTTLE_SECS` seconds and
grows exponentially on repeated breaches. Clear a peer's throttle state with:

```bash
net backpressure clear <peer_id>
```

Generate shell completions with:

```bash
net completions bash > /etc/bash_completion.d/net
source /etc/bash_completion.d/net
```

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
# {"difficulty":12345,"retune_hint":2,"timestamp_millis":1700000000000}
```

Prometheus metrics expose per-window EMAs for the Kalman retune:

```bash
curl -s localhost:9898/metrics | rg '^difficulty_window'
```

Counters `difficulty_window_short`, `difficulty_window_med`, and
`difficulty_window_long` track the short, medium, and long EMA windows
respectively.

## Example workloads

Sample workload descriptors live under `examples/workloads/`. Run one with:

```bash
cargo run --example run_workload examples/workloads/cpu_only.json
```

Replace the path with `gpu_inference.json` or `multi_gpu.json` to request GPU
providers. The example verifies the JSON schema and prints the derived
capability requirements.
