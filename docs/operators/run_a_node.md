# Run a node
> **Review (2025-09-30):** Added telemetry verification after overlay migration.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

## Hardware

- 4 cores
- 8 GB RAM
- SSD with at least 50 GB free


## Ports

- P2P: `3033` (TCP and UDP if using QUIC)
- RPC: `3030`
- Metrics: `9898`

## QUIC mode

Start the node with `--quic` to enable QUIC gossip alongside TCP. Runtime
configuration now lives in two files:

* `config/default.toml` retains port selection and legacy flags.
* `config/quic.toml` selects the transport provider and shared parameters for
  both Quinn and s2n. The loader merges this file on startup and every reload.

The following keys mirror the transport abstraction exposed by
`crates/transport`:

| Key | Description |
|-----|-------------|
| `provider` | Either `"quinn"` (default) or `"s2n-quic"`; determines which backend the registry instantiates. |
| `certificate_cache` | Optional path used for provider-managed certificates. Reuse the same path when switching providers so stored fingerprints survive. |
| `retry_attempts` / `retry_backoff_ms` | Listener/connection retry policy passed to the backend. |
| `handshake_timeout_ms` | Timeout before a connect attempt is treated as failed (default 5000 ms). |
| `rotation_history` | Number of historical fingerprints retained per provider (default 4). |
| `rotation_max_age_secs` | Maximum age of stored fingerprints in seconds (default 30 days). |

Successful connections increment `quic_provider_connect_total{provider}` so the
metrics dashboard reflects which implementation accepted a peer. CLI commands
(`blockctl net peers`, `blockctl net quic history`) now include the provider
identifier, fingerprint history, and most recent rotation timestamp. Use
`blockctl net quic stats --format table` (or `--json`) to inspect provider-labelled handshake latency, retransmits, and endpoint reuse without hitting the metrics endpoint. RPC
responses expose the same metadata for automation, and the cache stored in
`~/.the_block/quic_peer_certs.json` is partitioned by provider so history is
preserved when you swap backends. Reference the phase chart in
[`docs/pivot_dependency_strategy.md`](../pivot_dependency_strategy.md) before
changing providers so governance, telemetry, and simulation plans stay aligned.

Certificates continue to rotate automatically based on the TTL in
`config/default.toml`. Ensure the private key path remains owner-readable to
avoid remote peers rejecting the endpoint.

### Migrating between QUIC providers

To move from Quinn to s2n-quic (or back) without losing stored fingerprints:

1. Inspect the current cache via `blockctl net quic history --format table` to
   confirm recent rotations and note the provider column.
2. Ensure `config/quic.toml` points `certificate_cache` at a shared location (for
   example `state/quic_peer_certs.json`). Copy any existing cache into that
   path if it differs from the default.
3. Update `config/quic.toml` with `provider = "s2n-quic"` (or `"quinn"`) and,
   if required, adjust `handshake_timeout_ms` or the rotation policy. Run
   `blockctl config reload` or send `SIGHUP` so the node applies the new
   settings; the loader also reapplies them on restart.
4. Watch `quic_provider_connect_total{provider}` and the CLI history output to
   confirm new peers are attaching to the desired backend while previous
   fingerprints remain available for audit.

The cache retains separate histories for each provider, so you can roll back by
reverting the `provider` field and reloading without losing the prior chain.

## Quickstart

```sh
curl -LO <release-tar>
./scripts/verify_release.sh node-<ver>-x86_64.tar.gz checksums.txt checksums.txt.sig
mkdir -p ~/.block
tar -xzf node-<ver>-x86_64.tar.gz -C ~/.block
~/.block/node --datadir ~/.block/datadir --config ~/.block/config.toml
```

### Feature-gated CLI flags

The node binary now honours the workspace feature matrix so that light-weight
test builds do not have to link telemetry, gateway, or QUIC stacks unless they
are explicitly requested:

- `--auto-tune` requires building the binary with `--features telemetry`. When
  the feature is disabled the command exits with a clear
  `telemetry feature not enabled; --auto-tune unavailable` message instead of
  attempting to call into missing modules. Operators who want the historical
  CPU/memory tuning pass should compile with
  `cargo build -p the_block --features "cli telemetry" --bin node`.
- Supplying `--metrics-addr` without the `telemetry` feature now fails fast in
  the same way, preventing silent runs with missing Prometheus exporters.
- `--status-addr` spins up the HTTP status page only when the binary is built
  with the `gateway` feature. Plain builds print
  `gateway feature not enabled; status server unavailable` and continue
  without binding the port. Package the node with
  `cargo build -p the_block --features "cli gateway" --bin node` when the
  status endpoint is required.
- QUIC helpers (`--quic`, certificate rotation, chaos diagnostics) continue to
  live behind the `quic` feature. Non-QUIC builds ignore the flag rather than
  panicking when helper modules are missing.

The jurisdiction loader now records the language that ships with a policy pack
when calling `le_portal::record_action`, defaulting to English if the pack does
not specify one. Logs therefore capture both the region and localisation used
for legal hold records, mirroring the explorer's jurisdiction timeline.

## Config reload

The node watches `config/default.toml` for changes using the in-house runtime watcher and reloads rate-limit and reputation settings without restart. Trigger a manual reload with `config reload` from the CLI. Successful reloads increment `config_reload_total{result="ok"}` and update `config_reload_last_ts`; malformed files are logged and ignored.

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

Peer identifiers now use base58-check overlay IDs. The values printed by `net`
and returned from the RPC surfaces match the strings emitted by the gossip
relay and overlay diagnostics.

The CLI honours `peer_metrics_export` and `max_peer_metrics` configuration
limits. See [docs/gossip.md](../gossip.md) for protocol details and additional
RPC examples.

Backpressure limits derive from `p2p_max_per_sec` and `p2p_max_bytes_per_sec`
in `config.toml`; throttle duration defaults to `TB_THROTTLE_SECS` seconds and
grows exponentially on repeated breaches. Clear a peer's throttle state with:

```bash
net backpressure clear <peer_id>
```

### Migrating overlay peer stores

Nodes that previously persisted overlay peers in `~/.the_block/overlay_peers.json`
should migrate to the new JSON store under `~/.the_block/overlay/peers.json`
before upgrading. Run the bundled helper to perform a lossless conversion:

```bash
cargo run --bin migrate_overlay_store --release -- \
  ~/.the_block/overlay_peers.json ~/.the_block/overlay/peers.json
```

To migrate from a bespoke directory layout, pass explicit source and destination
paths:

```bash
cargo run --bin migrate_overlay_store --release -- \
  /var/lib/the-block/legacy_peers.json /srv/the-block/overlay/peers.json
```

The script accepts optional source/target paths and canonicalises every peer ID
to the base58-check representation. Existing directories are created on demand,
and the timestamp for each entry is set to the migration time so uptime probes
continue without manual edits. Governance release prep checklists in
[`docs/governance_release.md`](../governance_release.md) require completing this
step before staging overlay upgrades for quorum approval.

After migrating, confirm the overlay database contents and CLI output:

```bash
net overlay_status --format json | jq '.database_path'
net gossip_status
```

Both commands should report the new base58 peer identifiers and the target path
`~/.the_block/overlay/peers.json`.

After the CLI checks, confirm the telemetry probes picked up the refreshed
store. On nodes exposing the metrics endpoint (default `127.0.0.1:9898`), run:

```bash
curl -s http://127.0.0.1:9898/metrics | rg 'overlay_peer_(total|persisted_total)'
```

The `overlay_peer_total{backend="inhouse"}` and
`overlay_peer_persisted_total{backend="inhouse"}` gauges should reflect the new
peer count. Metrics-aggregator dashboards consume the same series, so a non-zero
value there is the final confirmation that rebate tracking and gossip fanout are
using the migrated data.

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
