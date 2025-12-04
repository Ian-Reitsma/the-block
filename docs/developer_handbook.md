# Developer Handbook

Every change assumes main-net readiness. Treat this as the working agreement for engineers and AI agents.

## Blockchain Concepts Cheat Sheet

If you're new to blockchain development, here's a quick reference:

| Concept | Plain English | Code/Docs |
|---------|---------------|-----------|
| **Block** | A batch of transactions bundled together and added to the chain every ~1 second | `node/src/blockchain`, [`architecture.md#ledger-and-consensus`](architecture.md#ledger-and-consensus) |
| **Transaction** | A signed message (transfer CT, store data, run compute, etc.) | `node/src/transaction.rs`, [`architecture.md#transaction-and-execution-pipeline`](architecture.md#transaction-and-execution-pipeline) |
| **Mempool** | The "waiting room" for transactions before they're included in a block | `node/src/mempool`, [`architecture.md#mempool-admission-and-eviction`](architecture.md#mempool-admission-and-eviction) |
| **Fee lane** | Priority tier (consumer, industrial, priority, treasury) affecting which transactions get included first | `node/src/fee`, [`economics_and_governance.md#fee-lanes-and-rebates`](economics_and_governance.md#fee-lanes-and-rebates) |
| **Subsidy bucket** | How new CT is allocated (storage, read, compute rewards) | `node/src/blockchain/block_binary.rs`, [`economics_and_governance.md#ct-supply-and-sub-ledgers`](economics_and_governance.md#ct-supply-and-sub-ledgers) |
| **Proposal** | A governance request to change parameters or spend treasury funds | `governance/src/proposals.rs`, [`economics_and_governance.md#proposal-lifecycle`](economics_and_governance.md#proposal-lifecycle) |
| **Macro-block** | Periodic checkpoint summarizing state for faster syncing | `node/src/macro_block.rs`, [`architecture.md#macro-blocks-and-finality`](architecture.md#macro-blocks-and-finality) |
| **SNARK** | Small proof that computation was done correctly (without re-running it) | `node/src/compute_market/snark.rs`, [`architecture.md#compute-marketplace`](architecture.md#compute-marketplace) |
| **Bridge** | Mechanism for moving assets between blockchains | `bridges/`, [`architecture.md#token-bridges`](architecture.md#token-bridges) |
| **Trust line** | Credit relationship between parties for DEX trading | `dex/`, [`architecture.md#dex-and-trust-lines`](architecture.md#dex-and-trust-lines) |
| **Read acknowledgement** | Proof that data was served to a client | `node/src/gateway/read_receipt.rs`, [`architecture.md#read-receipts`](architecture.md#read-receipts) |
| **Treasury disbursement** | Moving CT from community fund (requires governance vote) | `governance/src/treasury.rs`, [`economics_and_governance.md#treasury-and-disbursements`](economics_and_governance.md#treasury-and-disbursements) |
| **SimpleDb** | Our key-value store with crash-safe writes (atomic rename) | `node/src/simple_db.rs`, [`operations.md`](operations.md) |
| **Wrapper telemetry** | Metrics about which runtime/transport/storage providers are active | `node/src/telemetry.rs`, [`architecture.md#telemetry-and-instrumentation`](architecture.md#telemetry-and-instrumentation) |

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

> **For newcomers:** Here's what each test type does:
>
> | Test Type | What It Does | When to Run |
> |-----------|--------------|-------------|
> | **Unit tests** | Test individual functions in isolation | `just test-fast` — always before commits |
> | **Replay tests** | Re-run historical blocks to verify determinism (same input = same output, even on different CPUs) | `cargo test -p the_block --test replay` — when touching consensus/ledger |
> | **Settlement audit** | Double-entry accounting check — ensures CT doesn't magically appear or disappear | `cargo test -p the_block --test settlement_audit --release` — when touching economics |
> | **Fuzzing** | Throws random inputs at the code to find edge cases | `scripts/fuzz_coverage.sh` — for critical paths |
> | **Chaos tests** | Simulate failures (packet loss, disk full, network partition) | Specific test files — when touching networking/storage |

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
- Prover benchmarking harnesses live under `node/src/compute_market/tests/prover.rs` so you can run focused comparisons between CPU and GPU provers (`cargo test -p the_block prover_cpu_gpu_latency_smoke`).
- `tb-cli compute proofs --limit 10` calls `compute_market.sla_history` and prints proof fingerprints, backend selection, and circuit artifacts so you can validate end-to-end traces without spelunking the settlement sled DB.
- `tb-cli explorer sync-proofs --db explorer.db --url http://localhost:26658` takes the same RPC output, persists `Vec<ProofBundle>` records inside the explorer SQLite tables, and lets you re-verify bundles (or feed `/compute/sla/history`) without granting RPC access to dashboards.
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
- `scripts/deploy-worldos-testnet.sh` spins up the World OS energy stack (node + mock oracle + telemetry). Pair it with `docs/testnet/ENERGY_QUICKSTART.md` to exercise the `energy.*` RPCs locally.
- Use `tools/` for specialist binaries (settlement audit, peer-store migrator, etc.).

## Energy Market Development
- **Crates and modules** — `crates/energy-market` owns the provider/credit/receipt data model, metrics, and serialization; `node/src/energy.rs` persists the market via `SimpleDb` (sled under `TB_ENERGY_MARKET_DIR`, default `energy_market/`), exposes health checks, and records treasury accruals. RPC handlers live in `node/src/rpc/energy.rs`, CLI glue in `cli/src/energy.rs`, oracle ingestion under `crates/oracle-adapter`, and the mock oracle service in `services/mock-energy-oracle`.
- **Configuration** — Set `TB_ENERGY_MARKET_DIR` to relocate the sled DB (mirrors other `SimpleDb` consumers). Governance parameters (`energy_min_stake`, `energy_oracle_timeout_blocks`, `energy_slashing_rate_bps`) live in the shared `governance` crate; the runtime hooks call `node::energy::set_governance_params` so proposal activations atomically retune stakes, expiry, and slashing without code changes.
- **RPC and CLI flows** — `tb-cli energy register|market|settle|submit-reading` speak the same JSON schema the RPC expects (see `docs/apis_and_tooling.md#energy-rpc-payloads-auth-and-error-contracts`). Use `--verbose` or `--format json` to dump raw payloads for automation or explorer ingestion. Example round-trip:
  ```bash
  tb-cli energy register 10000 120 --meter-address meter_a --jurisdiction US_CA --stake 5000 --owner acct
  tb-cli energy market --provider-id energy-0x00 --verbose | jq .
  tb-cli energy submit-reading --reading-json @reading.json
  tb-cli energy settle energy-0x00 400 --meter-hash <hex> --buyer acct_consumer
  ```
- **Telemetry & metrics** — The crate emits `energy_providers_count`, `energy_avg_price`, `energy_kwh_traded_total`, `energy_settlements_total{provider}`, `energy_provider_fulfillment_ms`, and `oracle_reading_latency_seconds`. Gate pending-credit health via `node::energy::check_energy_market_health` logs; dashboards ingest the same metrics via the metrics-aggregator.
- **Testing** — Run `cargo test -p energy-market` for unit coverage and `cargo test -p node --test gov_param_wiring` to ensure governance parameters round-trip correctly. Use `scripts/deploy-worldos-testnet.sh` + `docs/testnet/ENERGY_QUICKSTART.md` for integration drills (node + mock oracle + telemetry). When altering serialization, add vectors under `crates/energy-market/tests` and extend the CLI tests in `cli/tests/` to keep JSON schemas stable.
- **Oracle adapters** — `crates/oracle-adapter` currently ships `NoopSignatureVerifier`; replacing it with the real verifier requires feeding Ed25519/Schnorr keys through env vars (`TB_ORACLE_SIGNING_KEY`, etc., to be finalised) and extending test vectors. The mock oracle service (`services/mock-energy-oracle`) exposes `/meter/:id/reading` and `/meter/:id/submit` endpoints over the in-house `httpd` router so you can simulate both fetching and submitting readings without third-party stacks.
- **Next steps** — Signature verification, dispute RPCs, explorer visualisations, and deterministic replay coverage are tracked in `docs/architecture.md#energy-governance-and-rpc-next-tasks` and summarised in `AGENTS.md`. Treat those bullets as blocking work items whenever you touch the energy crates.

## Contribution Flow
1. Open an issue or draft PR describing the change.
2. Create a feature branch, keep it rebased, and avoid merge commits.
3. Run `fmt`, `clippy`, `nextest`, relevant integration tests, and `mdbook build docs`.
4. Update docs (this handbook + subsystem sections) as part of the same PR.
5. Include test output + rationale in the PR description; mention any skipped suites and why.
