# Contract CLI Feature Flags
> **Review (2025-09-25):** Synced Contract CLI Feature Flags guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The `contract-cli` crate now exposes opt-in feature flags so developers can
compile lighter builds for test harnesses while enabling heavier integrations on
demand.

## Available Features

- `wasm-metadata` – pulls in `wasmtime` to extract exported symbols from WASM
  blobs during `contract deploy --wasm …`. When disabled the CLI still deploys
  contracts but omits the metadata attachment.
- `sqlite-storage` – enables the optional SQLite migration helpers from
  `foundation_sqlite` so the log commands can import legacy `.db` files into
  the first-party log store. The default build already ships the sled-backed
  store, so this flag is only required when upgrading historical archives.
- `full` – convenience feature that enables both `wasm-metadata` and
  `sqlite-storage`.

Other feature flags remain unchanged (`wallet`, `quantum`, `telemetry`).

## Recommended Workflows

- **Lean test builds**: `cargo test -p contract-cli` runs without enabling the
  heavy features to avoid linking Wasmtime and the bundled SQLite build. This
  keeps integration tests from exhausting memory on constrained runners.
- **Full CLI for operators**: enable the convenience feature when running the
  binary if you need the SQLite migrator: `cargo run -p contract-cli --features full -- logs search --db ./logs_store`.
  Combine it with other flags as needed, e.g. `--features "full quantum"` for
  Dilithium wallet commands.

Scripts or downstream tooling that rely on metadata extraction or log search
should add `--features full` (or the individual feature flags) to their Cargo
invocations.
