# Headless Node Installation
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

The `installer` utility packages signed binaries and provides an auto-update
mechanism suitable for servers or minimal environments.

## Packaging

```
cargo run --package installer -- package --os linux --out node.zip
```

This command checks common dependencies, zips the binaries and emits a
BLAKE3 signature alongside the archive.

## Auto-Update

Run the updater periodically to fetch signed releases and retain a rollback
copy of the previous binary:

```
cargo run --package installer -- update
```

## Headless Deployment

1. Download or build the archive on a build machine.
2. Verify the `.sig` file and extract the archive.
3. Run `the-block` with the desired configuration in a systemd service or
   your preferred init system.

The installer performs no GUI operations and is safe to use in remote
server environments.