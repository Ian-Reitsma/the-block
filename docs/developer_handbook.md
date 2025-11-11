# Developer Handbook

Every change assumes main-net readiness. Treat this as the working agreement for engineers and AI agents.

## Environment Setup
- Run `scripts/bootstrap.sh` (Linux/macOS) or `scripts/bootstrap.ps1` (Windows/WSL). The script installs Rust 1.86+, `cargo-nextest`, `cargo-fuzz`, Node 18+, Python 3.12.3 venv, and OS packages (`patchelf`, `llvm-tools-preview`).
- Set `PATH=.venv/bin:$PATH` to pick up the Python shim, and ensure `rustup show` lists the workspace toolchain.
- Optional: install `just`, `nix`, or `direnv` if you rely on those flows; the repo ships configs for each.

## Workspace Layout
- `node/` – full node, gateway, RPC, compute/storage stacks, plus Python bindings.
- `crates/` – first-party libraries (`foundation_*`, `transport`, `httpd`, `storage_engine`, `p2p_overlay`, `wallet`, `probe`, etc.).
- `cli/` – user-facing CLI with governance, wallet, bridge, compute, telemetry, and remediation commands.
- `metrics-aggregator/`, `monitoring/`, `explorer/` – ops tooling.
- `bridges/`, `dex/`, `storage_market/`, `gateway/` – specialised crates referenced by the node.
- `docs/` – this handbook (mdBook). Run `mdbook build docs` before submitting docs changes.

## Toolchain and Commands
- `just lint` → `cargo clippy --workspace --all-targets --all-features`.
- `just fmt` → `cargo fmt --all`.
- `just test-fast` → targeted unit tests; `just test-full` → `cargo test --workspace --features telemetry`.
- `make monitor`, `make aggregator`, and `make cli` wrap common workflows.
- Use `cargo nextest` for high-parallel test runs; CI uses the same harness.

## Coding Standards
- `#![forbid(unsafe_code)]` across the workspace. If you think you need `unsafe`, stop and open an issue.
- Prefer first-party crates (`httpd`, `foundation_tls`, `foundation_serialization`, `foundation_sqlite`, `storage_engine`) over upstream dependencies.
- Use `concurrency::{MutexExt, DashMap}` instead of raw locks to keep poisoning + metrics consistent.
- Keep modules small and feature-gated; RPC code should stay in `node/src/rpc`, CLI code in `cli/src`, etc.

## Testing Strategy
- Unit tests live next to code; integration tests under `node/tests`, `gateway/tests`, `bridges/tests`, etc.
- Replay harness: `cargo test -p the_block --test replay` replays ledger snapshots across architectures.
- Settlement audit: `cargo test -p the_block --test settlement_audit --release` must pass before merging.
- Fuzzing: `scripts/fuzz_coverage.sh` installs LLVM tools, runs fuzz targets (e.g., `cargo fuzz run storage`), and uploads `.profraw` artifacts. Remember to set `LLVM_PROFILE_FILE`.
- Chaos: `tests/net_gossip.rs`, `tests/net_quic.rs`, `node/tests/storage_repair.rs`, `node/tests/gateway_rate_limit.rs` simulate packet loss, disk-full, etc.

## Debugging and Diagnostics
- Enable `RUST_LOG=trace` plus the diagnostics subscriber when chasing runtime issues; `diagnostics::tracing` is wired everywhere.
- `cli/src/debug_cli.rs` and `tb-cli diagnostics …` provide structured dumps for mempool, scheduler, gossip, mesh, TLS, and telemetry state.
- Use `docs/operations.md#probe-cli-and-diagnostics` for probe commands.

## Performance and Benchmarks
- Bench harnesses sit under `benches/`, `monitoring/build`, and `node/benches`. Publish results through the metrics exporter by setting `TB_BENCH_PROM_PATH`.
- `docs/benchmarks.md` content moved here: store thresholds in `config/benchmarks/<name>.thresholds`, compare to `monitoring/metrics.json`, and watch Grafana’s **Benchmarks** row.

## Contract and VM Development
- WASM tooling: `cli/src/wasm.rs`, `node/src/vm`, `node/src/vm/debugger.rs`. Use `docs/architecture.md#virtual-machine-and-wasm` for runtime behaviour.
- `docs/contract_dev.md`, `docs/wasm_contracts.md`, and `docs/vm_debugging.md` merged here.
- CLI flow: `tb-cli wasm build`, `tb-cli contract deploy`, `tb-cli contract call`, `tb-cli vm trace`.
- Gas model (`node/src/vm/gas.rs`):
  - Each opcode has a base cost (`cost(op)`), and storage/hash-heavy ops add explicit constants (`GAS_STORAGE_READ`, `GAS_STORAGE_WRITE`, `GAS_HASH`).
  - `GasMeter` enforces limits and reports `used()` for fee accounting. ABI helpers (`node/src/vm/abi.rs`) encode `(gas_limit, gas_price)` as a 16-byte blob when interacting with wallets.
- Debugger + traces: `node/src/vm/debugger.rs` steps through opcodes, exposes `VmDebugger::into_trace()` (stack, gas, opcode, pc). CLI `tb-cli vm trace --tx <hash> --json` prints entries like:

  ```json
  {"pc":12,"opcode":"SSTORE","gas_before":1200,"gas_after":696,"stack":[1,2,3]}
  ```

  Use it alongside `tb-cli contract disasm` when diagnosing mispriced contracts.

## Python + Headless Tooling
- `demo.py` exercises the `node/src/py.rs` bridge for deterministic ledger replay and educational demos.
- Headless tooling (`docs/headless.md` content) stays in `cli/src/headless.rs` and `docs/apis_and_tooling.md`.

## Dependency Policy
- Policies live in `config/dependency_policies.toml`. Run `cargo run -p dependency_registry -- --check config/dependency_policies.toml` (or `just dependency-audit`) to refresh `docs/dependency_inventory*.json`.
- The pivot strategy formerly described in `docs/pivot_dependency_strategy.md` now reads: wrap critical stacks in first-party crates, record governance overrides, and track violations via telemetry + dashboards.
- Never introduce `reqwest`, `serde_json`, `bincode`, etc. Production crates must route through the first-party facades.

## Formal Methods and Verification
- Formal specs (`formal/*.fst`, `docs/formal.md`) integrate with CI. Run `make -C formal` or `cargo test -p formal` to re-check F* proofs before merging math-heavy changes.
- zk-SNARK and Dilithium proofs are stored alongside code; refer to `docs/maths/` for derivations.
- Simulation framework lives under `sim/`:
  - Scenarios are regular Rust binaries (see `sim/examples/basic.rs`, `sim/fee_spike.rs`, `sim/compute_market/*`). They accept `--scenario <name> --out <dir>` flags and emit JSON summaries (latency histograms, slashing events, etc.).
  - `cargo run -p sim -- --scenario dependency_fault --config sim/src/dependency_fault_harness/config.toml` reproduces dependency-fault drills. Logs land in `sim/target/`.
  - Use the harness before altering consensus/governance logic; CI expects new scenarios for major protocol toggles.

## Logging and Traceability
- Logging guidelines from `docs/logging.md` live here: use structured events, avoid PII, include `component`, `peer`, `slot`, `lane`, `job_id` labels.
- Traces feed into the metrics aggregator and optionally into external stacks via exporters (no vendor lock-in required).

## Explainability and AI Diagnostics
- `docs/explain.md` + `docs/ai_diagnostics.md` merged here. CLI commands `tb-cli explain tx|block|governance` render JSON traces; governance toggles `ai_diagnostics_enabled` to control ANN-based alerts.

## Developer Support Scripts
- `Justfile` targets include bootstrap, fmt/lint/test, docs, coverage, fuzz, and docker image builds.
- `scripts/` directory hosts installers, overlay-store migrations, settlement audits, chaos helpers, and release scripts.
- Use `tools/` for specialist binaries (settlement audit, peer-store migrator, etc.).

## Contribution Flow
1. Open an issue or draft PR describing the change.
2. Create a feature branch, keep it rebased, and avoid merge commits.
3. Run `fmt`, `clippy`, `nextest`, relevant integration tests, and `mdbook build docs`.
4. Update docs (this handbook + subsystem sections) as part of the same PR.
5. Include test output + rationale in the PR description; mention any skipped suites and why.
