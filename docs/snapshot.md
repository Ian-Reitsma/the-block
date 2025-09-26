# Snapshot and Restore
> **Review (2025-09-25):** Synced Snapshot and Restore guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The snapshot tool creates deterministic RocksDB archives for quick bootstrap.

## Creating a Snapshot
Use `snapshot create <db> <out.zst>` to produce a zstd-compressed archive with an embedded checksum. Metric `snapshot_created_total` counts successful operations.

## Restoring
`snapshot restore <archive.zst> <db>` reconstructs the database. Failures increment `snapshot_restore_fail_total`.

Governance controls the snapshot schedule via the `snapshot_interval` parameter.
