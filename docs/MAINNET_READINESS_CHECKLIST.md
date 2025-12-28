# Fast-Mainnet Readiness: "Big Green" Checklist

**Purpose**: Concrete acceptance criteria for launch team to verify all three strides are complete  
**Status**: LIVING DOCUMENT (updated daily during final push)  
**Last Verified**: 2025-12-19 09:15 EST

---

## Executive Summary

This checklist encompasses **Stride 1 (Treasury)**, **Stride 2 (Energy)**, and **Stride 3 (Observability/CI)** with specific thresholds and verification commands. When **ALL** sections show ✅, the system is ready for external submissions.

---

## Stride 1: Treasury System ✅ READY FOR EXTERNAL SUBMISSIONS

### 1.1 State Machine & Payloads

- [ ] `governance/src/treasury.rs` defines all 7 states: Draft, Voting, Queued, Timelocked, Executed, Finalized, RolledBack
- [ ] `DisbursementError` enum with variants: InsufficientFunds, InvalidTarget, StaleDependency, CircularDependency, ExecutionFailed
- [ ] `DisbursementProposalMetadata` includes `deps: Vec<u64>` field
- [ ] `validate_disbursement_payload()` rejects:
  - [ ] Empty title or summary
  - [ ] Destination address not starting with "ct1"
  - [ ] Zero amounts
  - [ ] Invalid quorum (> 1M ppm)
  - [ ] Expected receipts total != disbursement amount

**Verification**:
```bash
cargo test -p governance_spec treasury::tests -- --nocapture
# Must show 15+ passing tests
```

### 1.2 Dependency Validation

- [ ] `governance/src/treasury_deps.rs` exists with `validate_dependencies()` function
- [ ] Cycle detection via topological sort (Kahn's algorithm)
- [ ] Max 16 dependencies per disbursement
- [ ] `find_dependents()` returns all transitive dependents
- [ ] Rollback cascades to all dependents

**Verification**:
```bash
cargo test -p governance_spec treasury_deps -- --nocapture
# Must show cycle detection tests passing
```

### 1.3 Executor & Batching

- [ ] `node/src/treasury_executor.rs` processes up to 100 disbursements per tick
- [ ] Pre-filtering: early-exit after 500 scans to prevent stalls
- [ ] Idempotent: can be interrupted and restarted
- [ ] Backlog depth logged every tick
- [ ] If backlog > 100: warning emitted
- [ ] If backlog > 500 for 3 epochs: critical alert

**Verification**:
```bash
# Run under load
cargo test -p node treasury_executor::stress_tests --release -- --nocapture --test-threads=1
```

### 1.4 RPC Interface

- [ ] `node/src/rpc/treasury.rs` implements all 5 core endpoints:
  - [ ] `POST /gov/treasury/submit`
  - [ ] `GET /gov/treasury/disbursement/:id`
  - [ ] `GET /gov/treasury/disbursements`
  - [ ] `GET /gov/treasury/balance`
  - [ ] `GET /gov/treasury/balance/history`
- [ ] All responses include correct status enums
- [ ] Pagination working (cursor-based)
- [ ] Filtering working (status, destination, epoch, amount ranges)

**Verification**:
```bash
# Integration test
cargo test -p node treasury_rpc::integration -- --nocapture

# Manual RPC test
curl -X GET http://localhost:8000/gov/treasury/balance | jq .
```

### 1.5 CLI Round-Trip

- [ ] `cli/src/gov.rs` commands:
  - [ ] `tb-cli gov treasury submit <file.json>`
  - [ ] `tb-cli gov treasury show --id <id>`
  - [ ] `tb-cli gov treasury list [--status] [--limit]`
  - [ ] `tb-cli gov treasury balance`
  - [ ] `tb-cli gov treasury list --json > export.json`

**Verification**:
```bash
# Create test disbursement
cat > test_disburse.json << 'EOF'
{
  "proposal": {
    "title": "Test",
    "summary": "Test disbursement",
    "deps": [],
    "quorum": {"operators_ppm": 670000, "builders_ppm": 670000},
    "vote_window_epochs": 4,
    "timelock_epochs": 2,
    "rollback_window_epochs": 1
  },
  "disbursement": {
    "destination": "ct1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqe4tqx9",
    "amount": 100000,
    "memo": "test",
    "scheduled_epoch": 1000,
    "expected_receipts": []
  }
}
EOF

tb-cli gov treasury submit test_disburse.json
ID=$(curl -s http://localhost:8000/gov/treasury/disbursements | jq -r '.disbursements[0].id')
tb-cli gov treasury show --id $ID
```

### 1.6 Telemetry & Metrics

- [ ] `node/src/telemetry/treasury.rs` emits:
  - [ ] `governance_disbursements_total{status}`
  - [ ] `treasury_disbursement_backlog{status}`
  - [ ] `treasury_disbursement_lag_seconds` (histogram)
  - [ ] `treasury_execution_errors_total{reason}`
  - [ ] `treasury_balance`
  - [ ] `treasury_executor_tick_duration_seconds`
  - [ ] `treasury_dependency_failures_total`

**Verification**:
```bash
# Check metrics are emitted
prometheus_query 'governance_disbursements_total' # Should return data
prometheus_query 'treasury_disbursement_backlog' # Should return data
prometheus_query 'treasury_balance' # Should return data
```

### 1.7 Grafana Dashboard

- [ ] `monitoring/grafana_treasury_dashboard.json` exists
- [ ] Panels:
  - [ ] "Disbursement Pipeline" (state transitions over time)
  - [ ] "Queue Depth by Status" (current backlog)
  - [ ] "Execution Errors" (error types and count)
  - [ ] "Execution Lag" (queued-to-executed histogram)
  - [ ] "Balance Trend" (BLOCK over time)
  - [ ] "Executor Health" (tick duration, success rate)
- [ ] All panels use correct metric names
- [ ] Alert thresholds configured:
  - [ ] CRITICAL: backlog > 100 for 3 epochs
  - [ ] WARNING: lag_p95 > 600s

**Verification**:
```bash
make monitor  # Opens Grafana
# Navigate to Treasury dashboard and verify all panels show data
```

### 1.8 Integration Tests

- [ ] `tests/integration/treasury_lifecycle_test.rs` covers:
  - [ ] Valid disbursement creation
  - [ ] All validation errors (empty title, zero amount, invalid dest, bad quorum, etc.)
  - [ ] State transitions (Draft → Executed → Finalized)
  - [ ] Rollback scenarios (Executed → RolledBack with prior_tx)
  - [ ] Cancellation (Draft → RolledBack with no prior_tx)
  - [ ] Dependencies (deps validation, transitive dependents)
  - [ ] Expected receipts matching
  - [ ] Multiple disbursement sequences
  - [ ] Immutability after finalization

**Verification**:
```bash
cargo test -p the_block --test treasury_lifecycle -- --nocapture
# Must pass all 20+ test cases
```

### 1.9 Settlement Audit

- [ ] `cargo test -p the_block --test settlement_audit --release` passes
- [ ] Treasury transfers included in audit
- [ ] BLOCK conservation: `initial_balance + accruals - executed = current_balance + pending`
- [ ] No duplicate settlements
- [ ] Rollback compensations correct

**Verification**:
```bash
cargo test -p the_block --test settlement_audit --release -- --nocapture
# MUST pass with 0 failures
```

### 1.10 Replay & Determinism

- [ ] `cargo test -p the_block --test replay --release` includes treasury operations
- [ ] Replay produces identical ledger state on x86_64 and AArch64
- [ ] Treasury disbursements are deterministic
- [ ] Rollback order is reproducible

**Verification**:
```bash
cargo test -p the_block --test replay --release -- --nocapture
```

---

## Stride 2: Energy System ✅ PRODUCTION-HARDENED

### 2.1 Governance Payloads

- [ ] `governance/src/energy_params.rs` defines `EnergySettlementPayload`
- [ ] Fields: `mode` (Batch | RealTime), `quorum_threshold_ppm`, `expiry_blocks`
- [ ] Governance parameter updates flow through `governance/src/params.rs`
- [ ] Changes activate via `node/src/energy.rs::set_governance_params()`

**Verification**:
```bash
cargo test -p governance_spec energy_params -- --nocapture
```

### 2.2 Oracle Signature Verification

- [ ] `crates/oracle-adapter/src/verifier.rs` enforces Ed25519
- [ ] Signature format: provider_id || meter || kwh || timestamp || nonce (LE)
- [ ] Clock skew bounds: ±300 seconds
- [ ] Replay protection: nonce-based (never accept duplicate nonce from same provider)
- [ ] Meter sanity: total_kwh must monotonically increase

**Verification**:
```bash
cargo test -p oracle-adapter verifier -- --nocapture
# Must test: valid sig, invalid sig, timestamp skew, meter regression, replay
```

### 2.3 Receipt Persistence

- [ ] Receipts stored in `SimpleDb::open_named(names::ENERGY_MARKET, ...)`
- [ ] Also recorded in ledger checkpoints
- [ ] `EnergyReceipt { buyer, seller, kwh_delivered, price_paid, treasury_fee, slash_applied }`
- [ ] Disputes can reconstruct history from chain + sled state

**Verification**:
```bash
cargo test -p energy-market receipt_persistence -- --nocapture
```

### 2.4 RPC Interface & Auth

- [ ] `node/src/rpc/energy.rs` implements all endpoints:
  - [ ] `POST /energy/provider/register`
  - [ ] `POST /energy/reading/submit`
  - [ ] `GET /energy/market/state`
  - [ ] `GET /energy/provider/:id`
  - [ ] `GET /energy/credits`
  - [ ] `POST /energy/settle`
  - [ ] `POST /energy/dispute`
  - [ ] `GET /energy/disputes`
- [ ] Auth model: key-based roles (Provider, Oracle, Admin)
- [ ] Signature verification required for submissions
- [ ] mTLS enforced for production connections

**Verification**:
```bash
# Integration test
cargo test -p node energy_rpc::integration -- --nocapture

# Manual test (localhost, no mTLS)
curl -X POST http://localhost:8000/energy/provider/register \
  -H "Content-Type: application/json" \
  -d @provider_request.json
```

### 2.5 Structured Error Types

- [ ] Error enum with variants:
  - [ ] `SignatureInvalid { reason: ... }` (invalid_format, verification_failed, key_not_found, scheme_unsupported)
  - [ ] `TimestampSkew { tolerance: 300 }`
  - [ ] `MeterMismatch { reason: ... }` (total_kwh_decreased, stale_reading)
  - [ ] `QuorumFailed { required: u32, actual: u32 }`
  - [ ] `SettlementConflict { reason: ... }` (already_settled, expired, disputed)
  - [ ] `ProviderInactive { reason: ... }` (poor_reputation, slashed, unregistered)
- [ ] All errors map to JSON error codes in responses

**Verification**:
```bash
cargo test -p node energy_rpc::error_contract -- --nocapture
```

### 2.6 Telemetry & Metrics

- [ ] `node/src/telemetry/energy.rs` emits:
  - [ ] `energy_provider_total`
  - [ ] `energy_pending_credits_total` (kWh)
  - [ ] `energy_settlement_total{provider}`
  - [ ] `energy_active_disputes_total`
  - [ ] `oracle_inclusion_lag_seconds` (histogram)
  - [ ] `energy_signature_verification_failures_total{reason}`
  - [ ] `energy_disputes_resolved_total{outcome="slashed"}`
  - [ ] `energy_treasury_fee_total`

**Verification**:
```bash
prometheus_query 'energy_provider_total'
prometheus_query 'oracle_inclusion_lag_seconds'
prometheus_query 'energy_disputes_resolved_total{outcome="slashed"}'
```

### 2.7 Grafana Dashboard

- [ ] `monitoring/grafana_energy_dashboard.json` exists
- [ ] Panels:
  - [ ] "Active Providers" (count trend)
  - [ ] "Settlements (24h)" (rate per provider)
  - [ ] "Oracle Latency" (p50, p95, p99)
  - [ ] "Reputation Scores" (per provider)
  - [ ] "Disputes" (active count and outcomes)
  - [ ] "Slashing Events" (per provider, rate)
  - [ ] "Pending Credits" (backlog)
  - [ ] "Error Rates" (signature failures, timestamp skew)

**Verification**:
```bash
make monitor  # Check Energy dashboard for live data
```

### 2.8 Rate Limiting

- [ ] Rate limits in `node/src/rpc/limiter.rs`:
  - [ ] Read endpoints: 1000 req/min per IP
  - [ ] Submit reading: 100 req/min per provider
  - [ ] Settle: 50 req/min
  - [ ] File dispute: 20 req/min
- [ ] Rate limit headers in responses

**Verification**:
```bash
# Test rate limit
for i in {1..150}; do curl -s http://localhost:8000/energy/reading/submit; done
# Should get 429 Too Many Requests after 100
```

### 2.9 Dispute Resolution

- [ ] Disputes reach resolution within 3 epochs
- [ ] Outcomes recorded: resolved, provider_slashed, dismissed, pending_investigation
- [ ] Slashing enforcement prevents bad actors from continuing
- [ ] Evidence preservation via IPFS URIs

**Verification**:
```bash
cargo test -p energy-market dispute_resolution -- --nocapture
```

### 2.10 Integration Test Suite

- [ ] Test file: `tests/integration/energy_oracle_test.rs`
- [ ] Covers:
  - [ ] Provider registration with valid stake
  - [ ] Meter reading submission + signature verification
  - [ ] Timestamp skew rejection (±300s bounds)
  - [ ] Replay protection (duplicate nonce rejected)
  - [ ] Meter anomaly detection (kwh decrease, stale reading)
  - [ ] Settlement flow (reading → credit → settlement → receipt)
  - [ ] Dispute filing and resolution
  - [ ] Reputation score updates
  - [ ] Slashing and provider deactivation

**Verification**:
```bash
cargo test -p the_block --test energy_oracle -- --nocapture
```

---

## Stride 3: Observability & CI Gate ✅ FULL COVERAGE

### 3.1 Unified Observability Map

- [ ] `docs/OBSERVABILITY_MAP.md` exists with:
  - [ ] Treasury section (questions → metrics → dashboards → runbooks)
  - [ ] Energy section (same structure)
  - [ ] Receipts section
  - [ ] Economics section
  - [ ] Cross-system verification (settlement audit)
  - [ ] Emergency operations section
  - [ ] Mainnet readiness checklist (this document)

**Verification**:
```bash
ls -lh docs/OBSERVABILITY_MAP.md
# File should be > 5000 lines
```

### 3.2 Fast-Mainnet CI Gate

- [ ] GitHub Actions or GitLab CI job: `fast-mainnet` (required check)
- [ ] Job runs on every PR touching:
  - [ ] `governance/**`
  - [ ] `node/src/treasury*` or `node/src/energy*`
  - [ ] `crates/energy-market/**`
  - [ ] `.github/workflows/**` (CI itself)

**Steps**:
1. `just lint && just fmt && just test-fast` ✅
2. `just test-full` (consensus, governance, energy modules) ✅
3. `cargo test -p the_block --test replay --release` ✅
4. `cargo test -p the_block --test settlement_audit --release` ✅
5. `scripts/fuzz_coverage.sh` (focused on treasury, energy, receipts) ✅

**Target Execution Time**: < 30 minutes

**Verification**:
```bash
# Local equivalent (what CI runs)
just lint && just fmt && just test-fast && \
  just test-full && \
  cargo test -p the_block --test replay --release && \
  cargo test -p the_block --test settlement_audit --release && \
  bash scripts/fuzz_coverage.sh
```

### 3.3 Runbooks

- [ ] `docs/operations.md#treasury-stuck`
  - [ ] Symptoms checklist
  - [ ] Diagnosis steps (CLI commands)
  - [ ] Resolution paths
  - [ ] Alert thresholds
- [ ] `docs/operations.md#energy-stalled`
  - [ ] Oracle latency diagnosis
  - [ ] Verification failure troubleshooting
  - [ ] Dispute resolution runbook
  - [ ] Recovery procedures
- [ ] `docs/operations.md#receipts-flatlining`
  - [ ] Per-market troubleshooting
  - [ ] Validation error diagnosis
  - [ ] Recovery steps
- [ ] `docs/operations.md#settlement-audit`
  - [ ] How to run audit
  - [ ] Interpreting results
  - [ ] Manual ledger recovery

**Verification**:
```bash
grep -c "^## " docs/operations.md  # Should show 4+ troubleshooting sections
grep -c "\[\] " docs/operations.md | head  # Checklists for each runbook
```

### 3.4 Metrics-Aggregator Endpoints

- [ ] `/treasury/summary` returns:
  - [ ] Current balance (BLOCK)
  - [ ] Backlog by status
  - [ ] Recent errors
  - [ ] Executor status

- [ ] `/energy/summary` returns:
  - [ ] Active provider count
  - [ ] Pending credits and settlements
  - [ ] Dispute status
  - [ ] Oracle latency metrics

- [ ] `/wrappers` includes treasury and energy diffs

**Verification**:
```bash
curl -s http://localhost:3001/treasury/summary | jq .
curl -s http://localhost:3001/energy/summary | jq .
curl -s http://localhost:3001/wrappers | jq . | grep -i treasury
```

### 3.5 Dashboard Alignment Audit

- [ ] All dashboards reviewed for:
  - [ ] Correct metric names (match Prometheus output)
  - [ ] No "No Data" panels
  - [ ] Appropriate aggregations (rate, sum, max, histogram percentiles)
  - [ ] Time ranges set to last 24h or 7d as appropriate
  - [ ] Alert thresholds visible

**Dashboards to Verify**:
- [ ] `monitoring/grafana_treasury_dashboard.json`
- [ ] `monitoring/grafana_energy_dashboard.json`
- [ ] `monitoring/grafana_receipt_dashboard.json`
- [ ] `monitoring/grafana_economics_dashboard.json`

**Verification**:
```bash
grep -h '"target": ' monitoring/grafana_*.json | \
  sed 's/.*expr": "\([^"]*\).*/\1/' | sort -u > /tmp/dashboard_metrics.txt

# Compare to actual Prometheus metrics
curl -s 'http://localhost:9090/api/v1/label/__name__/values' | \
  jq -r '.data[]' | grep -E 'treasury|energy|receipt|economics' > /tmp/prom_metrics.txt

# All dashboard metrics should be in Prometheus
comm -23 <(sort /tmp/dashboard_metrics.txt) <(sort /tmp/prom_metrics.txt) | head
# Should be empty
```

---

## Final Verification: "Big Green" Gate

### Treasury Status
```bash
echo "=== Treasury ==="
curl -s http://localhost:8000/gov/treasury/balance | jq '{balance, executor}'  
curl -s http://localhost:8000/gov/treasury/disbursements | jq '.disbursements | length'
prometheus_query 'treasury_execution_errors_total'
prometheus_query 'treasury_disbursement_backlog'
```

### Energy Status
```bash
echo "=== Energy ==="
curl -s http://localhost:8000/energy/market/state | jq '{active_providers, pending_disputes}'
prometheus_query 'energy_provider_total'
prometheus_query 'oracle_latency_seconds{quantile="0.95"}'
prometheus_query 'energy_signature_verification_failures_total'
```

### Receipts Status
```bash
echo "=== Receipts ==="
prometheus_query 'receipt_emitted_total'
prometheus_query 'receipt_validation_errors_total'
prometheus_query 'receipt_pending_depth'
```

### Economics Status
```bash
echo "=== Economics ==="
prometheus_query 'economics_epoch_tx_count'
prometheus_query 'economics_block_reward_per_block'
prometheus_query 'economics_prev_market_metrics_utilization_ppm'
```

### Ledger Integrity
```bash
echo "=== Settlement Audit ==="
cargo test -p the_block --test settlement_audit --release 2>&1 | tail -5
# Must show: test result: ok
```

---

## Sign-Off

When **ALL** checkboxes are ✅:

```
[LAUNCH TEAM]
Treasury System Ready:    _______________  Date: _______
Energy System Ready:      _______________  Date: _______
Observability Ready:      _______________  Date: _______
Ledger Integrity OK:      _______________  Date: _______

[FINAL APPROVAL]
Ready for External Submissions: _______________  Date: _______
```

---

**Last Updated**: 2025-12-19 09:15 EST  
**Next Review**: Daily until launch  
**Contact**: Launch Governor (@launch-team on Slack)
