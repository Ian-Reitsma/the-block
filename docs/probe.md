# Probe CLI and Metrics Manual
> **Review (2025-09-25):** Synced Probe CLI and Metrics Manual guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The `probe` utility performs synthetic health checks against a node. It ships as
a standalone binary in `crates/probe` and supports multiple subcommands for
checking RPC responsiveness, gossip reachability, mining progress, and chain tip
height.

## Usage

```
probe [--timeout SECS] [--expect NUM] [--prom] <SUBCOMMAND>
```

Global flags:

- `--timeout` – maximum seconds to wait before declaring failure (default 5).
- `--expect` – expected value for latency or height depending on subcommand.
  Exceeding the threshold returns a timeout error.
- `--prom` – print first-party telemetry snapshots (`probe_success` and
  `probe_duration_seconds`) for the metrics aggregator.

Exit codes: `0` on success, `1` on error, `2` on timeout.

## Subcommands

### `PingRpc`
Sends a JSON-RPC `metrics` request to the node and measures round-trip
latency.

```
probe ping-rpc http://127.0.0.1:3050
```

### `MineOne`
Starts mining via the RPC interface and waits until tip height increases by at
least one block (or `--expect` value). Stops mining once the delta is reached.

### `GossipCheck`
Attempts a TCP connection to the gossip port to verify that peers can reach the
node.

### `Tip`
Fetches current block height from the metrics endpoint and prints it to stdout.
Fails if the height is below `--expect`.

## Metrics Output

With `--prom`, the probe emits:

```
probe_success 1
probe_duration_seconds 0.134
```

These counters feed the foundation metrics aggregator and surface in the
rendered dashboard without requiring Prometheus.

## Examples

```bash
# Expect RPC to respond within 200ms
probe --timeout 2 --expect 200 ping-rpc http://127.0.0.1:3050

# Verify gossip port and export metrics
probe --prom gossip-check 10.0.0.8:3030
```

## Development

The implementation lives in
[`crates/probe/src/main.rs`](../crates/probe/src/main.rs). Unit tests mock network
responses and confirm timeout paths. Operators can schedule probes via `cron` or
systemd timers and stream the snapshots to the foundation metrics aggregator.
