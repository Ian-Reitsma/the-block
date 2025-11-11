# The Block

The Block is a Rust-first proof-of-work + proof-of-service L1 targeting one-second blocks, notarised micro-shard roots, and a single transferable currency (CT). Everything shipping in `main` is first-party: networking over `crates/transport`, HTTP/TLS via `crates/httpd`, serialization through `foundation_serialization`, overlay via `p2p_overlay`, storage by `storage_engine`, and CLI/SDK tooling that reuses those stacks end-to-end.

## Mission
- Reward verifiable storage/compute/bandwidth by minting `STORAGE_SUB_CT`, `READ_SUB_CT`, and `COMPUTE_SUB_CT` directly in the coinbase, eliminating per-request billing.
- Keep governance deterministic: the `governance` crate powers the node, CLI, explorer, and metrics aggregator so proposals, fee floors, and treasury state never drift.
- Ship reproducible, dependency-sovereign binaries with telemetry that proves which runtime/transport/storage/coding providers are live.
- Offer first-party clients—gateway HTTP, DNS publishing, CLI, probe, explorer, light clients, and telemetry stacks—so operators can run an entire cluster without piecing together third-party software.

## Quick Start
```bash
# install toolchains, Python venv, Node 18, patchelf (Linux)
./scripts/bootstrap.sh

# lint/format/test (mirrors CI)
just lint && just fmt && just test-fast
# full suite with telemetry feature
just test-full

# start a local node with the default config
tb-cli node start --config node/.env.example
```
- Requirements: Rust 1.86+, `cargo-nextest`, `cargo-fuzz` (nightly), Python 3.12.3, Node 18+ (`npm ci --prefix monitoring` builds dashboards).
- Environment variables use the `TB_*` namespace; see `node/src/config.rs` for all knobs.

## Build & Test
- `cargo nextest run --all-features` covers the workspace (node, CLI, crates, metrics aggregator).
- Deterministic replay: `cargo test -p the_block --test replay`.
- Settlement audit: `cargo test -p the_block --test settlement_audit --release`.
- Fuzzing: `scripts/fuzz_coverage.sh` installs LLVM tools, runs fuzz targets, and exports `.profraw` reports.
- Monitoring stack: `npm ci --prefix monitoring && make monitor` renders Grafana + Prometheus dashboards; `metrics-aggregator` ingests `/metrics` endpoints and exposes `/wrappers`, `/governance`, `/treasury`, `/bridge`, `/probe`, `/chaos`, and `/remediation/*` APIs.

## Documentation
All former subsystem docs were consolidated into a concise handbook:

| Doc | Scope |
| --- | --- |
| [`docs/overview.md`](docs/overview.md) | Mission, design pillars, repo layout, document map. |
| [`docs/architecture.md`](docs/architecture.md) | Ledger/consensus, transaction pipeline, networking, storage, compute, bridges/DEX, gateway, telemetry. |
| [`docs/economics_and_governance.md`](docs/economics_and_governance.md) | CT supply, multipliers, fee lanes, treasury, governance DAG, service badges, settlement & audits. |
| [`docs/operations.md`](docs/operations.md) | Bootstrap, configuration, telemetry wiring, metrics aggregator, monitoring dashboards, probe/diagnostics, runbooks, WAL/snapshot care. |
| [`docs/security_and_privacy.md`](docs/security_and_privacy.md) | Threat model, cryptography, remote signers, privacy/KYC, jurisdiction packs, LE portal, supply-chain security. |
| [`docs/developer_handbook.md`](docs/developer_handbook.md) | Environment setup, tooling, testing/fuzzing, simulation, dependency policy, contract/WASM dev, contribution flow. |
| [`docs/apis_and_tooling.md`](docs/apis_and_tooling.md) | JSON-RPC, CLI, gateway HTTP & DNS, explorer, light-client streaming, storage APIs, probe CLI, metrics endpoints, schema references. |
| [`docs/LEGACY_MAPPING.md`](docs/LEGACY_MAPPING.md) | Per-file map showing where every removed doc landed. |

`mdbook build docs` renders the entire set; `docs/book.toml` keeps the configuration minimal.

## Key Components
| Area | Paths | Notes |
| --- | --- | --- |
| Node | `node/src/**` | Full node, gateway, mempool, compute/storage pipelines, RPC, telemetry. `#![forbid(unsafe_code)]` across crates. |
| Crates | `crates/transport`, `crates/httpd`, `crates/foundation_*`, `crates/storage_engine`, `crates/p2p_overlay`, `crates/wallet`, `crates/probe` | First-party libraries used by node, CLI, explorer, metrics, and tooling. |
| CLI | `cli/src/**` | `tb-cli` handles governance, wallet, bridge, compute, storage, telemetry, diagnostics, remediation, ANN/ad-market flows. |
| Metrics | `metrics-aggregator/`, `monitoring/` | Aggregates node metrics, publishes dashboards, enforces bridge remediation + TLS warning policy. |
| Tooling | `scripts/`, `tools/`, `examples/`, `sim/`, `fuzz/` | Bootstrap, dependency audit, settlement audit, chaos harness, simulation and fuzz suites. |

## Contributing
- **Read [`AGENTS.md`](AGENTS.md)** once and work like you wrote it—coding standards, testing gates, and review expectations live there.
- Follow the developer handbook for environment setup, dependency policy, logging, explainability tooling, and VM/contract workflows.
- Run `mdbook build docs` whenever documentation changes.
- Dependency audits: `cargo run -p dependency_registry -- --check config/dependency_policies.toml` refreshes `docs/dependency_inventory*.json`.
- Use `scripts/pre-commit.sample` to wire fmt/lint hooks; prefer `just` targets for repeatable workflows.

## License
Licensed under the Apache License, Version 2.0. See [`LICENSE`](LICENSE).
