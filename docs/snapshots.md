# Snapshot Rotation and CI Restore

Nodes periodically emit a full snapshot followed by incremental diffs. CI uses
`scripts/snapshot_ci.sh` to verify that the latest snapshot plus diffs reconstruct
the live state.

Snapshot operations export the following Prometheus metrics:

- `snapshot_duration_seconds` (histogram) – time spent creating or applying a snapshot.
- `snapshot_fail_total` (counter) – failures during snapshot operations.

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
