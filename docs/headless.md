# Headless Node Installation
> **Review (2025-09-25):** Synced Headless Node Installation guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The `installer` utility packages signed binaries and provides an auto-update
mechanism suitable for servers or minimal environments.

## Packaging

```
cargo run --package installer -- package --os linux --out node.tar.gz
```

The command now emits a deterministic `.tar.gz` archive backed by the
first-party compression stack and writes a matching BLAKE3 signature next to
the bundle.

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
