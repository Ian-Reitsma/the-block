# State Pruning and Compaction Guide
> **Review (2025-09-25):** Synced State Pruning and Compaction Guide guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

State grows monotonically as blocks commit new key/value pairs. To keep disk usage
bounded the node exposes a pruning subsystem that periodically drops historical
snapshots while retaining enough checkpoints for fast recovery.

## Retention Window Configuration

The pruning manager tracks snapshots under `state/` and preserves a sliding
window of the most recent checkpoints. The window length is controlled by the
`state.prune.keep` value in `config/node.toml`:

```toml
[state.prune]
keep = 4            # number of recent snapshots to retain
```

Setting `keep = 0` keeps no historical snapshots—the pruning pass will delete
every file after each checkpoint. Operators that need to retain everything
should configure a large window or pass the `--no-prune` CLI flag to disable the
pruner entirely.

## Snapshot Creation and Prune Cycle

`state/src/snapshot.rs` defines `SnapshotManager`, which serializes the Merkle
trie to disk after each block or on a configured interval. During `snapshot()`
execution the manager invokes `prune()` to remove snapshots older than the
retention window. The pruning pass sorts candidate files by their filesystem
modification time (newest first) and, when timestamps collide, falls back to the
filename for a deterministic order. Files beyond the configured `keep` count are
deleted from disk and removed from the checkpoint index.

### Snapshot Engine Migration

Snapshots record the storage engine that produced them via the optional
`engine_backend` field. When restoring into a node configured for a different
backend, `SnapshotManager::restore` appends an entry to
`state/engine_migrations.log` and forwards the event to the audit trail via
`state::audit::append_engine_migration`. To migrate an existing state
directory:

1. Stop the node and back up the entire `state/` tree.
2. Use the `tools/storage_migrate` binary to rewrite the RocksDB/sled
   directories into the target backend, e.g. `cargo run -p storage_migrate --
   state state.rocksdb rocksdb`. The tool verifies checksums before writing the
   destination.
3. Update `config/default.toml` so the `storage` engine defaults (or overrides
   for `state`-related handles) point at the new backend. The
   `storage_legacy_mode` toggle can be set to `true` for one release to retain
   the previous behaviour, but it is deprecated and the CLI issues warnings when
   enabled.
4. Restart the node and confirm the migration with `the-block state status`.
   Snapshot restores that cross engines emit warnings in the CLI and surface the
   `storage_engine_info{name="state",engine="rocksdb"}` metric for dashboards.

Every snapshot filename is the BLAKE3 hash of its root state. Restoring a node
is as simple as reading the snapshot and replaying blocks higher than the
checkpoint height.

## Block Retrieval After Pruning

Older blocks are not deleted; pruning only removes intermediate state. When a
pruned block is requested the node reconstructs the state by loading the closest
snapshot and replaying subsequent blocks from the ledger. This keeps RPCs like
`state.get` functional even when historic snapshots are gone.

## CLI Usage

Manual pruning and status inspection are available through the node CLI:

```bash
# remove snapshots beyond the configured window
$ the-block state prune

# show current snapshot directory, window size, and disk usage
$ the-block state status
```

These commands are idempotent and can be run while the node is offline. `state
status` exits with code 1 if snapshots are missing or corrupted.

## Metrics

Telemetry counters surface pruning behavior for monitoring:

| Metric Name           | Type      | Description                                |
|-----------------------|-----------|--------------------------------------------|
| `prune_duration_ms`   | histogram | time spent removing expired snapshots       |
| `pruned_bytes_total`  | counter   | cumulative bytes reclaimed from disk        |
| `snapshot_kept_total` | counter   | number of snapshots retained after pruning |

Grafana dashboards should alert if `prune_duration_ms` spikes or if
`pruned_bytes_total` remains zero despite high snapshot counts.

## Recovery and Over‑Pruning

If pruning removed a snapshot needed for audit or replay, rebuild the node by
copying a snapshot from another peer or by replaying the entire chain from
block 0. After restoring a replacement snapshot, restart the node with
`--no-prune` until the desired historical window is rebuilt.

Running `state prune --dry-run` first is recommended in critical environments to
list files that would be deleted without modifying disk state.
