# Snapshot Rotation and CI Restore

Nodes periodically emit a full snapshot followed by incremental diffs. CI uses
`scripts/snapshot_ci.sh` to verify that the latest snapshot plus diffs reconstruct
the live state.

## CI Validation

The script mines a few blocks, copies the generated snapshot and diffs to a new
location, restores the chain, and compares the account proof root against the
running instance. It exits non-zero if the roots diverge.

Run the check manually with:

```bash
scripts/snapshot_ci.sh
```
