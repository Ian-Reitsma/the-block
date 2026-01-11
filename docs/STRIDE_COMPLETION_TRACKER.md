# Three Big Strides — Completion Tracker

**Status**: IN PROGRESS  
**Started**: 2025-12-19  
**Target**: Fast-mainnet readiness for treasury, energy, and observability

---

## Executive Summary

This document tracks the completion of three critical workstreams required for mainnet launch:
- **Stride 1**: Treasury disbursement workflow ready for external submissions
- **Stride 2**: Energy governance and oracle controls production-hardened
- **Stride 3**: Unified observability and fast-mainnet CI gate

---

## Stride 1: Treasury Disbursement Workflow & Explorer Timelines

### Status: 🔄 IN PROGRESS (40% → 100%)

### 1.1 Spec + Doc Recon Pass ✅ COMPLETE

**Gap Analysis Results**:
1. ✅ State machine exists: Draft→Voting→Queued→Timelocked→Executed→Finalized/RolledBack
2. ❌ Missing: DAG dependency validation in executor
3. ❌ Missing: Explicit error state enum with recovery paths
4. ❌ Missing: Complete RPC contract documentation in docs/apis_and_tooling.md
5. ❌ Missing: Explorer JSON schema alignment
6. ❌ Missing: Metrics for queue depth, lag histograms, and error breakdowns
7. ⚠️  Partial: Telemetry hooks exist but dashboard wiring incomplete

### 1.2 End-to-End State Machine & Payloads 🔄 IN PROGRESS

**Files Being Modified**:
- [ ] governance/src/treasury.rs — Add DisbursementError enum, dependency validation
- [ ] governance/src/store.rs — Enhance metrics emission
- [ ] node/src/treasury_executor.rs — Idempotent executor with explicit error states
- [ ] docs/economics_and_governance.md — Update lifecycle diagrams

**Deliverables**:
- [ ] `DisbursementError` enum with variants: InsufficientFunds, InvalidTarget, StaleDependency, CircularDependency, ExecutionFailed
- [ ] Dependency DAG validator that checks proposal.deps before queueing
- [ ] Executor backlog stall detection (alert if pending > 100 for > 3 epochs)
- [ ] Lifecycle diagram with error transitions

### 1.3 RPC + CLI + Explorer Contract 🔄 IN PROGRESS

**Files Being Modified**:
- [ ] node/src/rpc/treasury.rs — Lock down RPC surface, add structured errors
- [ ] cli/src/gov.rs — Round-trip payload validation
- [ ] docs/apis_and_tooling.md — JSON schema documentation
- [ ] examples/governance/treasury_disbursement.json — Example payloads

**Deliverables**:
- [ ] JSON schemas for all 5 RPC methods (submit, fetch, queue, execute, rollback)
- [ ] CLI commands: `contract-cli gov treasury submit`, `treasury status`, `treasury export --json`
- [ ] Explorer canonical snapshot format documentation

### 1.4 Telemetry, Metrics-Aggregator, Dashboards 🔄 IN PROGRESS

**Files Being Created/Modified**:
- [ ] node/src/telemetry/treasury.rs (NEW) — Treasury-specific metrics module
- [ ] governance/src/treasury.rs — Emit metrics on all state transitions
- [ ] metrics-aggregator/src/wrappers/treasury.rs (NEW) — /treasury/summary endpoint
- [ ] monitoring/grafana_treasury_dashboard.json (NEW) — Treasury operations dashboard

**Required Metrics**:
- [ ] `governance_disbursements_total{status="draft|voting|queued|executed|finalized|rolled_back"}`
- [ ] `treasury_disbursement_backlog{status}`
- [ ] `treasury_disbursement_lag_seconds` (histogram: queued_at → executed_at)
- [ ] `treasury_execution_errors_total{reason="insufficient_funds|invalid_target|stale_dep|circular_dep|execution_failed"}`
- [ ] `treasury_balance`

### 1.5 Readiness Criteria & Tests/CI ⏳ PLANNED

**Files Being Created**:
- [ ] tests/integration/treasury_lifecycle_test.rs (NEW)
- [ ] tests/settlement_audit.rs — Extend for treasury verification
- [ ] scripts/ci/treasury_gate.sh (NEW)

**Test Coverage**:
- [ ] Full lifecycle: submit → approve → execute → verify ledger
- [ ] Rollback scenario: execute → detect issue → rollback → compensate
- [ ] DAG dependency: submit A, submit B(deps=[A]), verify B waits for A
- [ ] Backlog stress: 500 disbursements, verify batching and no stalls
- [ ] Settlement audit: verify BLOCK conservation across treasury operations

---

## Stride 2: Energy Governance, Oracle Controls, and Interfaces

### Status: ⏳ PLANNED (35% → 100%)

### 2.1 Spec-First Map of Energy Track 🔄 IN PROGRESS

**Design Note** (to be created): `docs/internal/energy_design_note.md`

**Spec vs Implementation Gaps**:
1. ✅ Bayesian reputation exists
2. ✅ Ed25519 signature infrastructure exists
3. ❌ No governance payload for batch vs real-time settlement
4. ❌ Quorum/expiry not enforced in RPC layer
5. ❌ Signature verification is stubbed in RPC
6. ❌ No structured error types for oracle failures
7. ❌ Auth model undefined

### 2.2 Governance Payloads & Oracle Verifier (15.G) ⏳ PLANNED

**Files to Create/Modify**:
- [ ] governance/src/energy_params.rs (NEW) — EnergySettlementPayload, QuorumSpec
- [ ] crates/energy-market/src/lib.rs — Receipt persistence to ledger
- [ ] crates/oracle-adapter/src/verifier.rs — Production Ed25519 enforcement
- [ ] node/src/energy.rs — Quorum and expiry checks

**Deliverables**:
- [ ] `EnergySettlementPayload { mode: Batch | RealTime, quorum_threshold_ppm, expiry_blocks }`
- [ ] Clock skew bounds (±300 seconds)
- [ ] Replay protection via nonce tracking
- [ ] Receipt persistence: sled + ledger checkpoint

### 2.3 RPC, Auth, Rate Limiting (15.H) ⏳ PLANNED

**Files to Modify**:
- [ ] node/src/rpc/energy.rs — Structured errors, auth enforcement
- [ ] node/src/rpc/limiter.rs — Energy-specific rate limits
- [ ] cli/src/energy/submit_reading.rs — Round-trip test
- [ ] docs/apis_and_tooling.md — Energy RPC section

**Auth Model**:
- Roles: Provider, Oracle, Admin
- Scopes: submit_reading, dispute, settle
- Keys: Ed25519 provider keys

**Error Types**:
- [ ] `SignatureInvalid`, `TimestampSkew`, `MeterMismatch`, `QuorumFailed`, `SettlementConflict`

### 2.4 Telemetry, Dashboards, State Drills (15.H/I) ⏳ PLANNED

**Files to Create**:
- [ ] node/src/telemetry/energy.rs (NEW)
- [ ] metrics-aggregator/src/wrappers/energy.rs (NEW)
- [ ] monitoring/grafana_energy_dashboard.json (NEW)
- [ ] docs/operations.md (add Energy Market Operations section)

**Required Metrics**:
- [ ] `energy_provider_total{status="active|inactive"}`
- [ ] `energy_pending_credits_total`
- [ ] `energy_active_disputes_total`
- [ ] `energy_settlements_total{provider}`
- [ ] `oracle_latency_seconds` (histogram)
- [ ] `energy_slashing_total{provider}`

### 2.5 Security, Supply Chain, CI Gate ⏳ PLANNED

**Files to Modify**:
- [ ] dependency_inventory.json — Verify energy-market, oracle-adapter
- [ ] scripts/fuzz_coverage.sh — Add energy receipt fuzzing
- [ ] .github/workflows/fast-mainnet.yml — Include energy tests

---

## Stride 3: Fast-Mainnet Observability & CI Gate

### Status: ⏳ PLANNED (20% → 100%)

### 3.1 Unify Observability Contracts ⏳ PLANNED

**Deliverable**: `docs/internal/observability_map.md`

**Mapping Structure**:
```
Question: "Are receipts flowing?"
├─ Metrics: receipt_emitted_total, receipt_validation_errors_total
├─ Dashboard: monitoring/grafana_receipt_dashboard.json
└─ Runbook: docs/operations.md#receipts-flatlining

Question: "Is treasury stuck?"
├─ Metrics: treasury_disbursement_backlog, treasury_execution_errors_total
├─ Dashboard: monitoring/grafana_treasury_dashboard.json
└─ Runbook: docs/operations.md#treasury-stuck
```

### 3.2 Metrics-Aggregator + Monitoring Alignment ⏳ PLANNED

**Audit Tasks**:
- [ ] Verify all AGENTS.md metrics are emitted
- [ ] Align dashboard panel names with actual Prometheus metrics
- [ ] Add treasury and energy dashboards
- [ ] Document alerting thresholds

**Files to Create/Modify**:
- [ ] monitoring/grafana_treasury_dashboard.json (NEW)
- [ ] monitoring/grafana_energy_dashboard.json (NEW)
- [ ] monitoring/grafana_receipt_dashboard.json — Verify alignment
- [ ] monitoring/grafana_economics_dashboard.json — Verify alignment

### 3.3 Fast-Mainnet CI Gate Design ⏳ PLANNED

**File**: `.github/workflows/fast-mainnet.yml` or `scripts/ci/fast_mainnet_gate.sh`

**Job Steps**:
1. `just lint && just fmt && just test-fast`
2. `just test-full` (consensus, governance, energy)
3. `cargo test -p the_block --test replay --release`
4. `cargo test -p the_block --test settlement_audit --release`
5. `scripts/fuzz_coverage.sh` (consensus, treasury, receipts, energy)

**Success Criteria**: All pass, no flaky tests, <30min execution time

### 3.4 Runbooks & Operational Stories ⏳ PLANNED

**Files to Modify**: `docs/operations.md`

**New Sections**:
- [ ] Treasury Stuck: Symptoms, diagnosis (CLI commands, Grafana panels), resolution
- [ ] Energy Stalled: Provider offline, oracle timeout, dispute resolution
- [ ] Receipts Flatlining: Market not emitting, validation failures, backlog clearing

### 3.5 Definition of "Big Green" ⏳ PLANNED

**File**: `MAINNET_READINESS_CHECKLIST.md` (NEW)

**Checklist Structure**:
```markdown
# Fast-Mainnet Readiness: Big Green

## Treasury System
- [ ] Backlog < 50 pending disbursements
- [ ] No proposals stuck in timelock > 7 days
- [ ] Explorer and CLI show identical state
- [ ] Settlement audit passes with treasury operations

## Energy System
- [ ] All provider SLOs green (latency < 5s, availability > 95%)
- [ ] Zero auth violations in last 24h
- [ ] Disputes resolving within 3 epochs
- [ ] Replay tests pass with energy receipts

## Receipts + Economics
- [ ] All 4 markets emitting receipts
- [ ] Economics metrics stable (±5% variance)
- [ ] Launch Governor economics gate reading expected values
- [ ] Receipt validation errors < 0.1%

## CI/Observability
- [ ] Fast-mainnet gate passing
- [ ] All dashboards accessible
- [ ] Runbooks tested
- [ ] No missing metrics from AGENTS.md
```

---

## Progress Summary

| Stride | Phase | Status | Files Changed | Completion |
|--------|-------|--------|---------------|------------|
| **Stride 1** | Docs | 🔄 | 4 | 50% |
| | Code | ⏳ | 8 | 25% |
| | Tests | ⏳ | 3 | 0% |
| | **Total** | **🔄** | **15** | **30%** |
| **Stride 2** | Docs | ⏳ | 3 | 10% |
| | Code | ⏳ | 12 | 5% |
| | Tests | ⏳ | 4 | 0% |
| | **Total** | **⏳** | **19** | **5%** |
| **Stride 3** | Docs | ⏳ | 5 | 5% |
| | Dashboards | ⏳ | 4 | 0% |
| | CI | ⏳ | 2 | 0% |
| | **Total** | **⏳** | **11** | **2%** |
| **OVERALL** | | **🔄** | **45** | **13%** |

---

## Next Actions (Execution Order)

### Immediate (Next 2 hours)
1. ✅ Create this tracker
2. 🔄 Update docs/economics_and_governance.md (treasury lifecycle)
3. 🔄 Create docs/apis_and_tooling.md#treasury-rpc
4. 🔄 Implement governance/src/treasury.rs enhancements

### Short-term (Hours 2-6)
5. Add node/src/telemetry/treasury.rs
6. Create monitoring/grafana_treasury_dashboard.json
7. Write tests/integration/treasury_lifecycle_test.rs
8. Complete Stride 1

### Medium-term (Hours 6-12)
9. Start Stride 2: Energy design note
10. Implement energy governance payloads
11. Add energy telemetry + dashboard
12. Complete Stride 2

### Final (Hours 12-16)
13. Start Stride 3: Create observability map
14. Build fast-mainnet CI gate
15. Write comprehensive runbooks
16. Create MAINNET_READINESS_CHECKLIST.md
17. Complete Stride 3

---

**Last Updated**: 2025-12-19 08:54 EST  
**Next Review**: After each stride completion  
**Target Completion**: 2025-12-20
