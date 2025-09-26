# Dependency Sovereignty Pivot
> **Review (2025-09-25):** Updated phase table after codec/crypto wrapper delivery and documented wrapper telemetry/governance integration milestones.

The dependency-sovereignty initiative formalises the founders’ directive to own
our substrate end-to-end. The registry, wrapper, and governance hooks promoted
throughout the handbook guarantee that upstream crates become optional
accelerants rather than hidden prerequisites. This guide captures the why, the
sequencing, and the cross-functional expectations so every engineering,
operator, and governance workflow stays aligned.

## Context and Rationale

- **Risk containment.** More than 800 third-party crates previously dictated our
  runtime, transport, storage, and cryptography semantics. A single upstream
  patch could invalidate our safety proofs or stall release cadences. Ownership
  eliminates that roulette.
- **Deterministic delivery.** Governance, CLI, explorer, telemetry, and wallet
  consumers all assume identical behaviour. We can only meet that expectation by
  controlling the APIs and release tempo of the underlying components.
- **Strategic leverage.** In-house implementations let us tune performance,
  telemetry labels, and failure envelopes without negotiating with external
  maintainers. They also strengthen the IP story and licensing options.
- **Operator trust.** Stakeholders need crisp answers when backends change.
  Provider IDs, codec versions, and engine selections now emit through telemetry
  and CLI/RPC endpoints so rollouts remain observable and reversible.

## Phase Breakdown

| Phase | Scope | Status | Notes |
|-------|-------|--------|-------|
| 1 | Inventory & policy via `tools/dependency_registry` | ✅ Complete | Baseline snapshot stored in `docs/dependency_inventory.json`; violations surface in CI and release scripts. |
| 2 | CI & tooling gates for dependency policy | ✅ Complete | `just dependency-audit` and GitHub Actions enforce policy drift checks. |
| 3 | Runtime wrapper (`crates/runtime`) | ✅ Complete | Tokio hidden behind first-party traits; stub backend available for tests. |
| 4 | Runtime adoption & Tokio linting | ✅ Complete | Workspace crates import the wrapper; disallowed Tokio methods enforced in lint configs. |
| 5 | QUIC transport abstraction (`crates/transport`) | ✅ Complete | Quinn and s2n encapsulated with provider registry, config, and telemetry adapters. |
| 6 | Provider introspection & handshake wiring | ✅ Complete | Node, CLI, RPC, and telemetry expose provider IDs and rotation metadata; `config/quic.toml` governs selection. |
| 7 | P2P overlay trait crate | ✅ Complete | Libp2p and stub backends ship behind `crates/p2p_overlay` with persistence, telemetry, and CLI selection. |
| 8 | Overlay enforcement & diagnostics | ✅ Complete | Lints block direct libp2p usage; telemetry panels and CLI/RPC stats expose backend health. |
| 9 | Storage-engine abstraction | ✅ Complete | RocksDB, sled, and in-memory engines wrap the shared trait with config-driven selection. |
| 10 | Storage migration tooling | 🚧 In Progress | Snapshot tooling stages via temp files; incentive marketplace DHT migrations next. |
| 11 | Coding crate for erasure/compression | ✅ Complete | `crates/coding` fronts erasure/compression with rollout gates and telemetry labels. |
| 12 | In-house fallback coder/compressor | ✅ Complete | XOR parity, RLE compression, and bench harness shipped with governance-controlled rollout toggles. |
| 13 | Crypto suite consolidation | ✅ Complete | `crates/crypto_suite` owns signatures, hashing, KDF, and Groth16 helpers with regression/bench coverage. |
| 14 | Suite adoption across node/CLI/wallet | ✅ Complete | Node, CLI, explorer, and wallet migrate to the suite with compatibility re-exports. |
| 15 | Codec abstraction for serde/bincode | ✅ Complete | `crates/codec` exposes named profiles, serde bridging macros, telemetry hooks, and corruption tests. |
| 16 | Wrapper telemetry & dashboards | ✅ Complete | Prometheus metrics, aggregator `/wrappers` endpoint, and Grafana dashboards chart backend selections and failures. |
| 17 | Dependency fault simulation harness | ⏳ Planned | Harness scaffolding in `sim/`; awaiting overlay/storage abstractions. |
| 18 | Governance-managed dependency policy | 🚧 In Progress | Param definitions reviewed; CLI wiring underway. |
| 19 | Release pipeline vendor syncs | ✅ Complete | Provenance scripts hash vendored trees and archive registry snapshots. |
| 20 | Pivot runbook & onboarding guide | ✅ Complete | This document plus README/AGENTS revisions deliver the narrative. |

## Operator & Contributor Checklist

1. **Run the registry audit.** `just dependency-audit` (or `make dependency-check`)
   must succeed before submitting code or release artifacts.
2. **Load the new QUIC config.** Copy `config/quic.toml`, select the Quinn or
   s2n provider, and verify telemetry via `blockctl net quic stats`.
3. **Track backend metrics.** Dashboards now emit
   `transport_provider_connect_total{provider}`, `runtime_backend_info`, and
   other wrapper gauges; alerts should key off these labels, not crate names.
4. **Reference wrappers only.** Tokio, Quinn, RocksDB, serde/bincode, libp2p,
   and crypto imports must route through the first-party crates. The lint suite
   blocks stragglers.
5. **Stage migrations deliberately.** Use the simulation harness (phase 17) and
   governance parameters (phase 18) to greenlight backend swaps. Operators should
   document overrides in runbooks and confirm telemetry before promoting a change.
6. **Document learnings.** Any deviation discovered while reviewing subsystem docs
   must be patched both in code and in the relevant guide before closing a task.

## Governance & Telemetry Integration

- Dependency policies surface as on-chain parameters with explorer timelines,
  CLI views (`blockctl gov policy list`), and RPC output for automation.
- Telemetry exports per-wrapper metrics; Grafana dashboards in `monitoring/`
  have been updated to chart backend selection, failure rates, and latency.
- Release provenance includes vendor tree hashes and registry snapshots so
  downstream consumers can audit the shipped dependency set.

## Documentation Consolidation

To reduce sprawl without losing context:

- `docs/SUMMARY.md` now funnels subsystem readers to a single pivot guide, and
  every `.md` in `docs/` carries a unified review banner with the latest audit
  date.
- Roadmap, progress, and supply-chain docs cross-link to this runbook so updates
  propagate from one source of truth.
- Operator guides point to shared telemetry labels and CLI commands instead of
  duplicating explanations.

## Next Steps

- Finalise storage migration automation (phase 10) so RocksDB↔sled swaps replay safely and incentive-backed DHT marketplace hooks can launch without manual intervention.
- Build and run the dependency fault simulation harness (phase 17) using the completed wrapper traits to rehearse provider outages before production incidents.
- Complete governance parameter plumbing (phase 18) so backend selection and rollout windows surface as voteable controls with explorer timelines and CLI validations.

Owners should treat this document as the canonical reference when planning work
or reviewing PRs for dependency-sensitivity. Update it alongside the related
code whenever a wrapper ships, a governance control lands, or an operator
procedure changes.
