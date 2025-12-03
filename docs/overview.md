# Overview

The Block is the unification layer for storage, compute, networking, and governance that turns verifiable work into CT rewards. Everything in the workspace is owned by the maintainers—no third-party stacks in consensus or networking—so the documentation describes what already ships in `main`, not a roadmap.

## Mission
- Operate a one-second base layer that notarizes micro-shard roots while keeping the L1 deterministic and audit-friendly.
- Pay operators for real service (`STORAGE_SUB_CT`, `READ_SUB_CT`, `COMPUTE_SUB_CT`) instead of speculative gas schedules.
- Treat governance as an engineering surface: the same crate powers the node, CLI, explorer, and telemetry so proposals, fee-floor policies, and service-badge status never drift.
- Ship first-party clients: the in-house HTTP/TLS stack (`crates/httpd` + `crates/transport`) fronts every RPC, gateway, and gossip surface, and dependency pivots move through governance before they land in production.

## Responsibility Domains
| Domain | Repository roots | In-flight scope |
| --- | --- | --- |
| Consensus & Ledger | `node/src/consensus`, `node/src/blockchain`, `bridges`, `ledger`, `poh` | Hybrid PoW/PoS leader schedule, macro-block checkpoints, Kalman retarget, ledger invariants, bridge proofs. |
| Serialization & Tooling | `crates/foundation_serialization`, `crates/codec`, `docs/spec/*.json` | Canonical binary layout, cross-language vectors, CLI/SDK adapters. |
| Cryptography & Identity | `crypto`, `crates/crypto_suite`, `node/src/identity`, `dkg`, `zkp`, `remote_signer` | Hash/signature primitives, DKG, commit–reveal, identity registries, PQ hooks. |
| Core Tooling & UX | `cli`, `gateway`, `explorer`, `metrics-aggregator`, `monitoring`, `docs/apis_and_tooling.md` | RPC & CLI surfaces, gateways, dashboards, probe CLI, release tooling. |

## Design Pillars
| Pillar | Enforcement | Evidence |
| --- | --- | --- |
| Determinism | `#![forbid(unsafe_code)]`, `codec::profiles`, ledger replay tests cross `x86_64`/`aarch64`. | `cargo test -p the_block --test replay` and mdBook specs under `docs/architecture.md`. |
| Memory & Thread Safety | First-party runtime, no `unsafe`, concurrency helpers in `crates/concurrency`. | `miri`/ASan gates in CI, locking helpers (`MutexExt`, `DashMap`) wrap every shared structure. |
| Portability | Build matrix (Linux glibc/musl, macOS, Windows/WSL) plus `scripts/bootstrap.*`. | `Justfile` + `Makefile` run the same steps locally and in CI; provenance signatures gate releases. |

## End-to-End Flow
1. **Ingress** – Gateways accept blobs and RPCs over the in-house `httpd` router, encrypt/store via `node/src/storage` and `storage_market` receipts, and emit signed `ReadAck` acknowledgements.
2. **Mempool & Scheduling** – `node/src/mempool` feeds the multi-lane scheduler (`node/src/scheduler.rs`) that batches consumer/industrial traffic, applies fee-floor policy, and records QoS counters.
3. **Consensus** – The hybrid PoW/PoS engine (`node/src/consensus`) enforces macro-block checkpoints, PoH ticks, VDF randomness, and difficulty retune while gossip/range-boost propagate blocks.
4. **Rewarding & Treasury** – Subsidy accounting, service-badge tracking, treasury streaming, and governance DAG state live in `node/src/governance` and the shared `governance` crate; snapshots stream through CLI, explorer, aggregates, and telemetry.
5. **Observability & Audits** – Runtime telemetry (`node/src/telemetry.rs`), the metrics aggregator, dashboards under `monitoring/`, and runbooks in `docs/operations.md` keep operators in sync with governance hooks and incident tooling.

## Repository Layout (live tree)
| Path | Highlights |
| --- | --- |
| `node/` | Full node, gateway stack, compute/storage/bridge/mempool modules, RPC server. |
| `crates/` | First-party libraries: transport, HTTP, serialization, overlay, runtime, coding/erasure, wallet SDKs. |
| `cli/` | `tb-cli` binary with governance, bridge, wallet, identity, compute, telemetry, and remediation commands. |
| `metrics-aggregator/` | Aggregates Prometheus-style metrics, publishes dashboards, verifies TLS & governance state. |
| `monitoring/` | Grafana/Prometheus templates and scripts (build via `npm ci --prefix monitoring`). |
| `storage_market/`, `dex/`, `bridges/`, `gateway/` | Dedicated crates for specialized subsystems referenced throughout the docs. |
| `docs/` | The consolidated handbook you are reading (mdBook enabled). |

## Energy Market Snapshot
- **Code surface** — `crates/energy-market` implements providers, credits, receipts, and telemetry; `node/src/energy.rs` persists them in sled (`SimpleDb::open_named(names::ENERGY_MARKET, …)`), applies governance hooks, and exposes health checks. RPC handlers live in `node/src/rpc/energy.rs`, the CLI entry point is `cli/src/energy.rs`, and oracle ingestion goes through `crates/oracle-adapter` plus the `services/mock-energy-oracle` binary used by the World OS drill.
- **State & persistence** — Energy state is serialized with `foundation_serialization::binary::{encode,decode}` and stored wherever `TB_ENERGY_MARKET_DIR` points (default `energy_market/`). Snapshots occur after every mutation, mirroring the fsync+rename workflow the rest of `SimpleDb` uses so restarts replay identical providers/credits/receipts. Governance parameters (`energy_min_stake`, `energy_oracle_timeout_blocks`, `energy_slashing_rate_bps`) share the same proposal pipeline as other params; once a proposal activates, `node::energy::set_governance_params` updates the runtime config and re-snapshots the sled DB.
- **RPC & CLI** — The JSON-RPC namespace exposes `energy.register_provider`, `energy.market_state`, `energy.submit_reading`, and `energy.settle`. Requests use the exact schema documented in `docs/apis_and_tooling.md#energy-rpc-payloads-auth-and-error-contracts`, including the shared `MeterReadingPayload` used by oracle adapters, CLI tooling, and explorers. `tb-cli energy` prints tabular output by default, toggles JSON via `--verbose`/`--format json`, and pipes raw payloads to automation without diverging from the node schema.
- **Observability & operations** — Runtime metrics include gauges (`energy_providers_count`, `energy_avg_price`), counters (`energy_kwh_traded_total`, `energy_settlements_total{provider}`), and histograms (`energy_provider_fulfillment_ms`, `oracle_reading_latency_seconds`). `node::energy::check_energy_market_health` logs warnings when pending credits pile up or settlements stall. `docs/testnet/ENERGY_QUICKSTART.md` plus `scripts/deploy-worldos-testnet.sh` describe the canonical bootstrap procedure (node + mock oracle + telemetry stack); `docs/operations.md#energy-market-operations` extends the runbook with backup, dispute, and alerting guidance.
- **Security & governance alignment** — The outstanding work (oracle signature enforcement, dispute RPCs, explorer timelines, QUIC chaos drills, sled snapshot drills, release-provenance gates) is tracked in `docs/architecture.md#energy-governance-and-rpc-next-tasks` and summarized in `AGENTS.md`. `docs/security_and_privacy.md#energy-oracle-safety` documents key hygiene, secret sourcing, and telemetry redaction requirements for oracle adapters.

## Reference Workflow
1. Read `AGENTS.md` and this overview once—then work like you wrote them.
2. Run `scripts/bootstrap.sh` (or `.ps1`) to install Rust 1.86+, `cargo-nextest`, Python 3.12.3 venv, and toolchain shims.
3. Use `just lint`, `just fmt`, `just test-fast`, and `just test-full` to stay in sync with CI.
4. Keep dependency policy artifacts (`docs/dependency_inventory*.json`) up to date via `cargo run -p dependency_registry` or `just dependency-audit`.
5. Wire telemetry locally: `metrics-aggregator`, `monitoring/`, and `crates/probe` exercise the same endpoints operators rely on.

## Document Map
All remaining detail sits in six focused guides:
- [`docs/architecture.md`](architecture.md) — ledger, networking, storage, compute, bridges, gateway, telemetry.
- [`docs/economics_and_governance.md`](economics_and_governance.md) — CT supply, fees, treasury, proposals, service badges, kill switches.
- [`docs/operations.md`](operations.md) — bootstrap, deployments, telemetry wiring, dashboards, runbooks, chaos & recovery.
- [`docs/security_and_privacy.md`](security_and_privacy.md) — threat modelling, cryptography, remote signer flows, jurisdiction policy packs, LE portal, supply-chain controls.
- [`docs/developer_handbook.md`](developer_handbook.md) — environment setup, coding standards, testing/fuzzing, simulation, dependency policy, tooling.
- [`docs/apis_and_tooling.md`](apis_and_tooling.md) — JSON-RPC, CLI, gateway HTTP & DNS, explorer, probe CLI, metrics endpoints, schema references.

For historical breadcrumbs the removed per-subsystem files now redirect through [`docs/LEGACY_MAPPING.md`](LEGACY_MAPPING.md).
