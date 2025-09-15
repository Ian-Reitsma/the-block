# Snapshot and Restore

The snapshot tool creates deterministic RocksDB archives for quick bootstrap.

## Creating a Snapshot
Use `snapshot create <db> <out.zst>` to produce a zstd-compressed archive with an embedded checksum. Metric `snapshot_created_total` counts successful operations.

## Restoring
`snapshot restore <archive.zst> <db>` reconstructs the database. Failures increment `snapshot_restore_fail_total`.

Governance controls the snapshot schedule via the `snapshot_interval` parameter.
