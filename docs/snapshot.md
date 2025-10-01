# Snapshot and Restore
> **Review (2025-09-30):** Updated snapshot tooling docs for the hybrid `lz77-rle` compressor.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The snapshot tool creates deterministic RocksDB archives for quick bootstrap.

## Creating a Snapshot
Use `snapshot create <db> <out.lz77>` to produce an archive compressed with the in-house `lz77-rle` codec. Metric `snapshot_created_total` counts successful operations.

## Restoring
`snapshot restore <archive.lz77> <db>` reconstructs the database. Failures increment `snapshot_restore_fail_total`.

Governance controls the snapshot schedule via the `snapshot_interval` parameter.
