# World OS Specification — Overview

## Intent
World OS layers civic-scale services on top of the existing The-Block kernel. The spec consolidates the code already in this repository—`node/`, `ledger/`, `storage_market/`, `compute_market/`, `governance/`, `metrics-aggregator/`, and their supporting crates—plus the new physical-resource vertical and testnet plan requested here.

## Document Map
| File | Summary |
| --- | --- |
| `00-overview.md` | This document. Goals, invariants, and canonical sources. |
| `01-core-l1.md` | Consensus, sharding, fee routing, and RPC/state mapping for the core node. |
| `02-service-credits.md` | Storage/compute/bandwidth subsidy ledgers, read receipts, and settlement workflows. |
| `03-governance-treasury.md` | Bicameral voting, proposal lifecycle, and the treasury executor hooks in `node/src/treasury_executor.rs`. |
| `04-markets.md` | Ad/ANN market, compute/storage offer surfaces, telemetry, and CLI flows. |
| `05-jurisdiction-packs.md` | Regional policy packs from `crates/jurisdiction` and their enforcement path in RPC and CLI. |
| `06-physical-resource-layer.md` | Energy/bandwidth/hardware credit architecture and oracle adaptation instructions. |
| `ROADMAP.md` | Sequence for shipping the World OS public testnet. |

## Architecture Snapshot
- **Execution surface** — `node/src/consensus`, `node/src/blockchain`, `node/src/transaction`, and `ledger/` remain the canonical block/state machines. The PoH tick generator (`node/src/poh.rs`) feeds the fork-choice logic in `node/src/blockchain/process.rs`.
- **Mempool + fee lanes** — `node/src/mempool` maintains BLOCK-fee, industrial, and governance lanes. Fee accounting uses `node/src/fee` helpers along with the shared `FeeLane` enum in `node/src/transaction.rs`.
- **Markets** — `storage_market/` persists replica incentives via the sled-backed engine, `node/src/compute_market/*` orchestrates lane-aware job matching, and `crates/ad_market` exposes ANN/ad slots across CLI/RPC.
- **Governance** — `governance/` hosts the sled-backed `GovStore`. RPC endpoints live in `node/src/rpc/governance.rs` and propagate to CLI subcommands under `cli/src/governance.rs`. Treasury distribution uses the executor in `node/src/treasury_executor.rs` and ledger coinbase helpers in `ledger/`.
- **Telemetry** — The metrics stack wires `foundation_metrics` gauges inside each subsystem and publishes dashboards through `metrics-aggregator/` and `monitoring/`. Every new surface (energy, oracle) must export counters via the same registry.
- **Docs** — Architecture, economics, and developer workflows stay in `docs/`. This World OS spec links each component back to the canonical code, providing a landing zone for the physical-resource roadmap.

## Expectations
1. Every behavior described here must map to code or configuration checked into this repository. If the doc references a missing switch, patch the code or open an issue before diverging from AGENTS.md.
2. Specs default to The-Block’s serialization and telemetry stacks (`foundation_serialization`, `foundation_metrics`). New crates introduced here (`energy-market`, `oracle-adapter`) consume those same utilities.
3. RPC and CLI references track the names in `node/src/rpc` and `cli/src`. When adding new endpoints, update both the spec and the gateway/client docs under `docs/` in the same change.

## Reading Order
Start with `01-core-l1.md` to understand the chain/kernel responsibilities, then move outward (service credits → governance/treasury → markets → jurisdiction packs → physical resources → roadmap). Each component file calls out:
- canonical modules and data structures,
- RPC/CLI entry points,
- storage layout and telemetry,
- settlement/flow diagrams,
- open integration tasks for the energy-credit vertical.

Treat this directory as living spec—it must change in lockstep with code.
