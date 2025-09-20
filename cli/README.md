# Contract CLI Feature Flags

The `contract-cli` crate now exposes opt-in feature flags so developers can
compile lighter builds for test harnesses while enabling heavier integrations on
demand.

## Available Features

- `wasm-metadata` – pulls in `wasmtime` to extract exported symbols from WASM
  blobs during `contract deploy --wasm …`. When disabled the CLI still deploys
  contracts but omits the metadata attachment.
- `sqlite-storage` – bundles `rusqlite` with the SQLite amalgamation to power
  `contract logs` subcommands. Without the feature the CLI prints a helpful
  message directing operators to rebuild with SQLite support.
- `full` – convenience feature that enables both `wasm-metadata` and
  `sqlite-storage`.

Other feature flags remain unchanged (`wallet`, `quantum`, `telemetry`).

## Recommended Workflows

- **Lean test builds**: `cargo test -p contract-cli` runs without enabling the
  heavy features to avoid linking Wasmtime and the bundled SQLite build. This
  keeps integration tests from exhausting memory on constrained runners.
- **Full CLI for operators**: enable the convenience feature when running the
  binary: `cargo run -p contract-cli --features full -- logs search --db logs.db`.
  Combine it with other flags as needed, e.g. `--features "full quantum"` for
  Dilithium wallet commands.

Scripts or downstream tooling that rely on metadata extraction or log search
should add `--features full` (or the individual feature flags) to their Cargo
invocations.
