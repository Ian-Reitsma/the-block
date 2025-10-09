# Snapshot Rotation and CI Restore
> **Review (2025-09-25):** Synced Snapshot Rotation and CI Restore guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Nodes periodically emit a full snapshot followed by incremental diffs. CI uses
`scripts/snapshot_ci.sh` to verify that the latest snapshot plus diffs reconstruct
the live state.

Snapshot operations export the following runtime telemetry metrics:

- `snapshot_duration_seconds` (histogram) – time spent creating or applying a snapshot.
- `snapshot_fail_total` (counter) – failures during snapshot operations.
- `snapshot_interval` (gauge) – current interval in blocks.
- `snapshot_interval_changed` (gauge) – last requested interval via RPC.

The default Grafana dashboard includes **Snapshot Duration** (90th percentile)
and **Snapshot Failures** panels so operators can watch latency and errors in
real time.

## CI Validation

The script mines a few blocks, copies the generated snapshot and diffs to a new
location, restores the chain, and compares the account proof root against the
running instance. It exits non-zero if the roots diverge.

Run the check manually with:

```bash
scripts/snapshot_ci.sh
```

## Runtime reconfiguration

Adjust the snapshot interval at runtime via JSON-RPC. Intervals below 10 blocks
are rejected.

```bash
curl -s -X POST 127.0.0.1:3030 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"set_snapshot_interval","params":{"interval":1200}}'
```

The new value is persisted to `node-data/config.toml` so restarts honour the
updated interval.

```toml
# node-data/config.toml
snapshot_interval = 1200
price_board_path = "state/price_board.v2.bin"
price_board_window = 100
price_board_save_interval = 30
```

### Troubleshooting

- **Interval too small** – requests below 10 blocks return `{"error":{"message":"interval too small"}}` and leave the existing value unchanged.
- **Defaults after corruption** – if `config.toml` is unreadable the node falls back to the compile-time default (`1024`).

See [AGENTS.md](../AGENTS.md#17-agent-playbooks--consolidated) for contributor guidance when modifying snapshot code.
