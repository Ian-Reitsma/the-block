# Dependency Sovereignty Pivot
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

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
| 7 | P2P overlay trait crate | 🚧 In Progress | Discovery/uptime modules mapped out; libp2p adapters next to move. |
| 8 | Overlay enforcement & diagnostics | ⏳ Planned | Lints and telemetry stubs defined; waiting on phase 7 completion. |
| 9 | Storage-engine abstraction | 🚧 In Progress | Trait design under review; RocksDB/sled adapters scheduled next sprint. |
| 10 | Storage migration tooling | ⏳ Planned | Depends on phase 9 shipping. |
| 11 | Coding crate for erasure/compression | ⏳ Planned | API skeleton drafted; awaiting dependency audit sign-off. |
| 12 | In-house fallback coder/compressor | ⏳ Planned | Performance benchmarks scoped; requires phase 11. |
| 13 | Crypto suite consolidation | 🚧 In Progress | Signer/verifier wrappers prototyped; SNARK helpers queued. |
| 14 | Suite adoption across node/CLI/wallet | ⏳ Planned | Commences after phase 13 lands. |
| 15 | Codec abstraction for serde/bincode | 🚧 In Progress | Configurable profiles outlined; telemetry hooks pending. |
| 16 | Wrapper telemetry & dashboards | ⏳ Planned | Metrics schema defined; awaiting phases 7–15. |
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

- Finish the overlay, storage-engine, coding, crypto, and codec abstractions,
  then extend telemetry and governance hooks across each wrapper.
- Rehearse dependency failure drills once the simulation harness is wired to the
  new traits.
- Continue pruning third-party exposures until every critical path has an
  in-house implementation or a governed fallback.

Owners should treat this document as the canonical reference when planning work
or reviewing PRs for dependency-sensitivity. Update it alongside the related
code whenever a wrapper ships, a governance control lands, or an operator
procedure changes.
