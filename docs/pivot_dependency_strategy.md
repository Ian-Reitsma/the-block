# Pivot Dependency Strategy Runbook
> **Review (2025-09-25):** Synced Pivot Dependency Strategy Runbook guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This runbook aligns engineering and operations on the hybrid dependency strategy that
pairs runtime-selectable wrappers with policy-governed in-house fallbacks. It expands on
the dependency provenance requirements documented in [`docs/provenance.md`](provenance.md)
and the governance-controlled backend policies introduced in [`docs/governance.md`](governance.md).
Release provenance tooling now vendors the workspace into a staging directory, records
deterministic hashes in `provenance.json` and `checksums.txt`, and blocks tagging unless
dependency_registry snapshots match policy so this runbook focuses on operational rollout
coordination rather than hash attestation plumbing.

## 1. Strategy Overview

* **Hybrid abstraction:** Every critical dependency (runtime, transport, overlay, storage,
coding, crypto, codec) is accessed through wrapper crates that expose a stable trait
surface while allowing multiple backend implementations.
* **Policy guardrails:** Governance parameters (`RuntimeBackend`, `TransportProvider`,
`StorageEnginePolicy`, etc.) gate which backends can be activated. Operators apply policy
updates during release provenance checks so deployed binaries always reflect the approved
matrix.
* **Fallback readiness:** In-house implementations stay buildable and tested behind the
same wrappers. Simulation harnesses exercise failover paths and publish readiness
summaries, letting governance stage cutovers with confidence.
* **Tooling integration:** Release provenance scripts generate dependency snapshots,
vendor hashes, and policy attestations. The `tools/vendor_sync` helper keeps the vendored
tree reproducible, while `tools/dependency_registry` records policy-compliant snapshots
for auditing.

## 2. Wrapper Flow Diagram

```mermaid
graph TD
    subgraph Runtime
        R[Runtime Wrapper]
        RB[Runtime Backends]
    end
    subgraph Transport
        T[Transport Wrapper]
        TB[Quinn | s2n | Mock]
    end
    subgraph Overlay
        O[Overlay Wrapper]
        OB[Libp2p | Stub]
    end
    subgraph Storage
        S[Storage Engine Wrapper]
        SB[RocksDB | sled | InMemory]
    end
    subgraph Coding
        C[Coding Wrapper]
        CB[Erasure | Compression | Encryption]
    end
    subgraph Crypto
        CR[Crypto Wrapper]
        CRB[dalek | FFI | In-house]
    end
    subgraph Codec
        CO[Codec Wrapper]
        COB[Binary | JSON | Custom]
    end

    R --> T --> O --> S
    R --> C
    C --> CR
    CR --> CO

    classDef wrapper fill:#1f2933,stroke:#0ea5e9,stroke-width:2px,color:#f1f5f9;
    classDef backend fill:#0f172a,stroke:#334155,stroke-width:1px,color:#e2e8f0;
    class R,T,O,S,C,CR,CO wrapper;
    class RB,TB,OB,SB,CB,CRB,COB backend;
```

The arrows show control flow between wrappers. Each wrapper reads governance policy to
select an allowed backend before providing functionality to downstream crates.

## 3. Governance & Policy Integration

1. Operators or governance delegates draft proposals using `cli gov dependency-policy`
   commands. Policies enumerate approved backends and rollout windows.
2. On-chain voting ratifies changes; the node configuration loader observes overrides via
   the governance store.
3. Telemetry events emitted from `node/src/telemetry.rs` (see `docs/telemetry.md`) surface
   activated backends, divergence warnings, and governance-triggered switches.
4. Release workflows block tagging until dependency snapshots match policy baselines.
5. Explorer governance views display history so stakeholders can audit backend evolution.

## 4. Onboarding Checklist for Engineers

- [ ] Clone the repository and run `tools/vendor_sync` to populate the vetted vendor
      tree.
- [ ] Execute `cargo run -p dependency_registry -- --help` to inspect registry commands
      and practice generating a local snapshot.
- [ ] Review `docs/governance.md` (policy lifecycle) and `docs/provenance.md` (release
      attestation flow).
- [ ] Study wrapper crates (`runtime`, `transport`, `overlay`, `storage_engine`, `coding`,
      `crypto`, `codec`) to understand trait contracts and feature flags.
- [ ] Run the simulation harness (`cargo run -p sim --features wrappers`) to observe
      fallback activation metrics; compare with guidance in `docs/simulation_framework.md`.
- [ ] Read `docs/networking.md`, `docs/storage_erasure.md`, and `docs/telemetry.md` for
      subsystem-specific integration notes.

## 5. Operator Runbook for Backend Switches

1. **Pre-check:**
   - Confirm governance approval and rollout window (`cli gov status --detail`).
   - Verify dependency registry snapshot: `dependency_registry --snapshot current.json`.
   - Compare against published `checksums.txt` and vendor hash from the latest release.
2. **Activation:**
   - Update node configuration to reference the approved backend label.
   - Restart with `--governance-policy` pointing to the fetched snapshot if running in
     air-gapped environments.
   - Monitor telemetry dashboards (`/wrappers`, `/governance`) for switch confirmation.
3. **Validation:**
   - Call RPC `governance.dependency_policies` to confirm active settings.
   - Use `cli gov telemetry watch --backend` to stream switch events.
   - Cross-check external monitoring of vendor hashes (operations maintain the feed per
     coordination notes in `docs/provenance.md`).
4. **Rollback:**
   - Apply the staged fallback policy, restart nodes, and notify governance to log the
     emergency action.

## 6. Troubleshooting

| Symptom | Likely Cause | Diagnostic Steps | Resolution |
| --- | --- | --- | --- |
| Backend mismatch warning | Local config diverges from governance snapshot | `cli gov policy diff` | Reapply approved snapshot; rerun provenance checks |
| Policy violation during release | Snapshot missing or hash mismatch | Inspect `scripts/release_provenance.sh` output; rerun `dependency_registry --snapshot` | Regenerate snapshot, ensure vendor tree is synced, rerun release script |
| Telemetry silent after switch | Telemetry feature disabled or exporter misconfigured | Confirm `telemetry` feature flag; check `docs/telemetry.md` exporters | Enable telemetry feature and restart exporter |
| Simulation harness fails fallback readiness | In-house backend drifted | Run `cargo test -p sim -- --ignored fallback` and inspect logs | Patch backend, regenerate registry entry, update roadmap |
| Governance proposal rejected | Invalid backend identifier | Validate against `governance/src/params.rs` enums | Use approved identifiers, resubmit proposal |

## 7. Simulation Harness Summary

The simulation harness exercises wrapper failover logic under controlled fault injection:

* Run via `cargo run -p sim --features wrappers,fallback-testing`.
* Outputs readiness scores and drift deltas; interpret using thresholds documented in
  `docs/simulation_framework.md`.
* Persisted reports inform governance proposals and release notes. Store artifacts with
  release provenance for traceability.

## 8. Roadmap & Milestones

| Milestone | Owners | Checklist |
| --- | --- | --- |
| Dependency Registry Hardening | Release Eng | Snapshot validation in CI · External hash monitoring online · Registry CLI docs complete |
| Wrapper Convergence | Runtime/Transport Leads | Trait parity across crates · Telemetry coverage · Integration tests for fallbacks |
| Migration Tooling | Platform Eng | vendor_sync automation · Upgrade guides in `docs/deployment_guide.md` |
| Fallback Readiness | Reliability Eng | Simulation harness thresholds met · Incident drills logged |
| Governance Integration | Governance WG | ParamKey coverage · Explorer visibility · Bootstrap script rolled out |

Track progress in `docs/roadmap.md` and update each checklist during milestone reviews.

## 9. Feedback Loop & Presentation Plan

* Present this runbook at the next engineering sync; capture action items and annotate a
  feedback log in `docs/pivot_dependency_strategy.md`.
* File follow-up issues for any blockers uncovered during the presentation.
* Revisit the runbook monthly to incorporate telemetry insights, new backends, or policy
  changes.

## 10. Reference Materials

- [`docs/governance.md`](governance.md)
- [`docs/provenance.md`](provenance.md)
- [`docs/networking.md`](networking.md)
- [`docs/storage_erasure.md`](storage_erasure.md)
- [`docs/telemetry.md`](telemetry.md)
- [`docs/simulation_framework.md`](simulation_framework.md)
- [`docs/deployment_guide.md`](deployment_guide.md)
- [`docs/roadmap.md`](roadmap.md)

Maintaining dependency independence requires cross-team coordination. Keep wrappers
consistent, policy artifacts auditable, and fallback readiness visible so the pivot stays
coherent across releases.
