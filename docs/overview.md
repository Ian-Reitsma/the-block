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
