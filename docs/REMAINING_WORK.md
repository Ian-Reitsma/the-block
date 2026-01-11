# Remaining Work for Three Big Strides Completion

**Status**: 87% Complete (13 of 15 critical files delivered)  
**Estimated Time**: 2-3 hours  
**Blocker for Launch**: Grafana dashboards + CI gate configuration  

---

## Critical Path Items (Must Complete Before External Submissions)

### 1. Grafana Treasury Dashboard (45 minutes)

**File**: `monitoring/grafana_treasury_dashboard.json`

**Required Panels** (reference: `docs/OBSERVABILITY_MAP.md` Treasury section):

```json
{
  "panels": [
    {
      "title": "Disbursement Pipeline",
      "targets": ["governance_disbursements_total{status=~\".*\"}"],
      "type": "graph",
      "time_range": "24h",
      "description": "State transitions over time (draft->voting->queued->timelocked->executed->finalized/rolled_back)"
    },
    {
      "title": "Queue Depth by Status",
      "targets": ["treasury_disbursement_backlog{status=~\".*\"}"],
      "type": "stat",
      "description": "Current count of pending disbursements"
    },
    {
      "title": "Execution Lag (p95)",
      "targets": ["histogram_quantile(0.95, treasury_disbursement_lag_seconds_bucket)"],
      "type": "stat",
      "threshold": {"warning": 300, "critical": 600},
      "description": "Time from queued to executed (seconds)"
    },
    {
      "title": "Execution Errors",
      "targets": ["rate(treasury_execution_errors_total{reason=~\".*\"}[5m])"],
      "type": "graph",
      "description": "Error rate by reason (insufficient_funds, invalid_target, stale_dep, circular_dep, execution_failed)"
    },
    {
      "title": "Balance Trend",
      "targets": ["treasury_balance"],
      "type": "graph",
      "time_range": "7d",
      "description": "Treasury BLOCK balance over time"
    },
    {
      "title": "Executor Health",
      "targets": ["histogram_quantile(0.95, treasury_executor_tick_duration_seconds_bucket)"],
      "type": "stat",
      "threshold": {"warning": 1, "critical": 5},
      "description": "Time to process one tick (seconds)"
    }
  ]
}
```

**Verification**:
```bash
# After creating the JSON file
make monitor
# Navigate to Treasury dashboard
# Verify all 6 panels show data from Prometheus
```

---

### 2. Grafana Energy Dashboard (45 minutes)

**File**: `monitoring/grafana_energy_dashboard.json`

**Required Panels** (reference: `docs/OBSERVABILITY_MAP.md` Energy section):

```json
{
  "panels": [
    {
      "title": "Active Providers",
      "targets": ["energy_provider_status"],
      "type": "stat",
      "description": "Current count of active energy providers"
    },
    {
      "title": "Settlements (24h)",
      "targets": ["rate(energy_settlements_total{provider=~\".*\"}[24h])"],
      "type": "table",
      "description": "Settlement rate per provider (last 24 hours)"
    },
    {
      "title": "Oracle Latency",
      "targets": [
        "histogram_quantile(0.50, oracle_latency_seconds_bucket)",
        "histogram_quantile(0.95, oracle_latency_seconds_bucket)",
        "histogram_quantile(0.99, oracle_latency_seconds_bucket)"
      ],
      "type": "graph",
      "threshold": {"warning": 10, "critical": 30},
      "description": "Verification latency distribution (p50, p95, p99)"
    },
    {
      "title": "Reputation Scores",
      "targets": ["energy_reputation_score"],
      "type": "graph",
      "time_range": "7d",
      "description": "Provider reputation over time (0.0-1.0 scale)"
    },
    {
      "title": "Active Disputes",
      "targets": ["energy_active_disputes_total"],
      "type": "stat",
      "threshold": {"warning": 20, "critical": 50},
      "description": "Current count of unresolved disputes"
    },
    {
      "title": "Slashing Events",
      "targets": ["rate(energy_slashing_total{reason=~\".*\"}[24h])"],
      "type": "graph",
      "description": "Slashing rate per provider and reason (24h)"
    },
    {
      "title": "Pending Credits",
      "targets": ["energy_pending_credits_total"],
      "type": "stat",
      "description": "Pending energy credits awaiting settlement (kWh)"
    },
    {
      "title": "Signature Failures",
      "targets": ["rate(energy_signature_verification_failures_total{reason=~\".*\"}[5m])"],
      "type": "graph",
      "threshold": {"critical": 1},
      "description": "Signature verification failure rate per reason"
    }
  ]
}
```

**Verification**:
```bash
make monitor
# Navigate to Energy dashboard
# Verify all 8 panels show data
# Check alert thresholds are visible
```

---

### 3. Fast-Mainnet CI Gate Job (45 minutes)

**Option A: GitHub Actions** (recommended)

**File**: `.github/workflows/fast-mainnet.yml`

```yaml
name: Fast-Mainnet Gate

on:
  pull_request:
    paths:
      - 'governance/**'
      - 'node/src/treasury*'
      - 'node/src/energy*'
      - 'node/src/rpc/treasury.rs'
      - 'node/src/rpc/energy.rs'
      - 'crates/energy-market/**'
      - 'crates/oracle-adapter/**'
      - '.github/workflows/fast-mainnet.yml'

jobs:
  fast-mainnet:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      
      - name: Cache cargo registry
        uses: actions/cache@v3
        with:
          path: ~/.cargo/registry
          key: ${{ runner.os }}-cargo-registry-${{ hashFiles('**/Cargo.lock') }}
      
      - name: Cache cargo index
        uses: actions/cache@v3
        with:
          path: ~/.cargo/git
          key: ${{ runner.os }}-cargo-git-${{ hashFiles('**/Cargo.lock') }}
      
      - name: Cache cargo build
        uses: actions/cache@v3
        with:
          path: target
          key: ${{ runner.os }}-cargo-build-target-${{ hashFiles('**/Cargo.lock') }}
      
      - name: Lint
        run: just lint
      
      - name: Format Check
        run: just fmt --check
      
      - name: Test (Fast)
        run: just test-fast
        timeout-minutes: 10
      
      - name: Test (Full - Consensus, Governance, Energy)
        run: |
          cargo test -p governance_spec --release
          cargo test -p node consensus -- --nocapture --release
          cargo test -p node governance -- --nocapture --release
          cargo test -p node energy -- --nocapture --release
          cargo test -p energy-market --release
          cargo test -p oracle-adapter --release
        timeout-minutes: 15
      
      - name: Replay Test
        run: cargo test -p the_block --test replay --release -- --nocapture
        timeout-minutes: 10
      
      - name: Settlement Audit
        run: cargo test -p the_block --test settlement_audit --release -- --nocapture
        timeout-minutes: 10
      
      - name: Fuzz Coverage (Critical Paths)
        run: |
          bash scripts/fuzz_coverage.sh \
            --target consensus,treasury,receipts,energy \
            --duration 120
        timeout-minutes: 5
      
      - name: Upload Coverage
        if: always()
        uses: actions/upload-artifact@v3
        with:
          name: fuzz-coverage-${{ github.sha }}
          path: |
            target/coverage/
            .profraw
          retention-days: 30
```

**Option B: GitLab CI** (alternative)

**File**: `.gitlab-ci.yml` (add fast-mainnet job)

```yaml
fast-mainnet:
  stage: verify
  rules:
    - if: '$CI_MERGE_REQUEST_IID && $CI_COMMIT_BRANCH != "main"'
    - if: '$CI_COMMIT_BRANCH == "main"'
  script:
    - just lint
    - just fmt --check
    - just test-fast
    - cargo test -p governance_spec --release
    - cargo test -p node consensus -- --nocapture --release
    - cargo test -p node governance -- --nocapture --release
    - cargo test -p node energy -- --nocapture --release
    - cargo test -p the_block --test replay --release
    - cargo test -p the_block --test settlement_audit --release
    - bash scripts/fuzz_coverage.sh --target treasury,energy,receipts --duration 120
  artifacts:
    paths:
      - target/coverage/
      - .profraw
    expire_in: 30 days
  timeout: 30m
```

**Verification**:
```bash
# Local equivalent (run this before committing)
just lint && just fmt && just test-fast && \
  cargo test -p governance_spec --release && \
  cargo test -p node consensus --release && \
  cargo test -p node governance --release && \
  cargo test -p node energy --release && \
  cargo test -p the_block --test replay --release && \
  cargo test -p the_block --test settlement_audit --release

# Verify coverage
ls -lh target/coverage/
```

---

## Medium Priority Items (Operational Readiness)

### 4. Operations Runbooks (1 hour)

**File**: `docs/operations.md` (add sections)

**Template for Each Runbook**:

```markdown
## Treasury Stuck

### Symptoms
- [ ] `treasury_disbursement_backlog > 50` for 2+ epochs
- [ ] `treasury_disbursement_lag_seconds_p95 > 300`
- [ ] Executor `last_error != null`
- [ ] Stale disbursements (created_at > 3 days ago, still Queued)

### Diagnosis

**Step 1**: Check executor health
```bash
tb-cli gov treasury balance | jq .executor
# Look for: last_error, pending_matured, staged_intents
```

**Step 2**: List stuck disbursements
```bash
tb-cli gov treasury list --status queued | jq '.[] | select(.created_at < (now - 3600))'
```

**Step 3**: Check for dependency failures
```bash
tb-cli gov treasury list --status queued | while read id; do
  tb-cli gov treasury show --id $id | jq '.proposal.deps'
done
```

**Step 4**: Inspect error logs
```bash
grep treasury_executor /var/log/node/*.log | tail -50
```

**Step 5**: Check treasury balance
```bash
tb-cli gov treasury balance
# If insufficient: wait for accruals or governance approval
```

### Resolution

**If dependency issue**:
```bash
# Show dependency state
tb-cli gov treasury show --id <DEPENDENCY_ID>

# If dependency is RolledBack:
#   Cancel the dependent disbursement manually
tb-cli gov treasury rollback --id <STUCK_ID> --reason "Dependency failed"
```

**If insufficient funds**:
```bash
# Wait for treasury accruals (check treasury balance)
watch -n 30 'tb-cli gov treasury balance | jq .balance'

# Or request governance approval for fund allocation
```

**If executor error**:
```bash
# Restart executor
systemctl restart the-block

# Monitor recovery
watch -n 5 'tb-cli metrics summary | grep treasury_disbursement_backlog'
```

### Alert Thresholds
- CRITICAL: `treasury_disbursement_backlog > 100` for 3+ epochs OR executor fails
- WARNING: `treasury_disbursement_lag_seconds_p95 > 600`

---

## Energy Stalled

### Symptoms
- [ ] `oracle_latency_seconds_p95 > 10`
- [ ] `energy_signature_verification_failures_total` increasing rapidly
- [ ] `energy_pending_credits_total` not decreasing
- [ ] Provider readings not settling

### Diagnosis

[Similar detailed steps with CLI commands]

---

## Receipts Flatlining

### Symptoms
- [ ] `receipt_emitted_total` flat for 5+ blocks
- [ ] One or more markets (storage, compute, energy, ad) not emitting
- [ ] `receipt_validation_errors_total` increasing

### Diagnosis

[Per-market troubleshooting steps]

---

## Settlement Audit

### How to Run
```bash
cargo test -p the_block --test settlement_audit --release -- --nocapture
```

### Interpreting Results

[Explanation of BLOCK conservation, rounding errors, deduplication]
```

**Verification**:
```bash
# Verify runbook content
grep -c "^## " docs/operations.md  # Should show 4+ new sections
grep -c "bash" docs/operations.md   # Should have 20+ CLI command examples
```

---

## Optional: Energy Governance Payloads (Skip for Phase 1)

**File**: `governance/src/energy_params.rs` (advanced implementation)

Define energy settlement parameters for governance control:

```rust
pub struct EnergySettlementPayload {
    pub mode: EnergyMode,  // Batch vs RealTime
    pub quorum_threshold_ppm: u32,
    pub expiry_blocks: u64,
}

pub enum EnergyMode {
    Batch { blocks_per_settlement: u64 },
    RealTime { latency_bound_ms: u64 },
}
```

Integration:
1. Parse in `governance/src/params.rs`
2. Activate in `node/src/energy.rs::set_governance_params()`
3. Test via `cargo test -p governance_spec energy_params`

---

## Verification Script

**File**: `scripts/verify_metrics_coverage.sh`

```bash
#!/bin/bash
# Verify all AGENTS.md metrics are emitted

METRICS=(
  "governance_disbursements_total"
  "treasury_disbursement_backlog"
  "treasury_disbursement_lag_seconds"
  "treasury_execution_errors_total"
  "treasury_balance"
  "treasury_balance"
  "treasury_executor_tick_duration_seconds"
  "energy_provider_total"
  "energy_pending_credits_total"
  "energy_active_disputes_total"
  "energy_settlements_total"
  "oracle_latency_seconds"
  "energy_signature_verification_failures_total"
  "energy_slashing_total"
  "receipt_emitted_total"
  "receipt_validation_errors_total"
  "economics_epoch_tx_count"
  "economics_block_reward_per_block"
)

echo "Verifying metrics coverage..."
missing=0

for metric in "${METRICS[@]}"; do
  count=$(curl -s 'http://localhost:9090/api/v1/query' \
    --data-urlencode "query=$metric" | jq '.data.result | length')
  
  if [ "$count" -eq 0 ]; then
    echo "MISSING: $metric"
    ((missing++))
  else
    echo "OK: $metric ($count series)"
  fi
done

echo ""
if [ $missing -eq 0 ]; then
  echo "✓ All metrics present!"
  exit 0
else
  echo "✗ Missing $missing metrics"
  exit 1
fi
```

**Run**:
```bash
bash scripts/verify_metrics_coverage.sh
```

---

## Execution Checklist

- [ ] Create `monitoring/grafana_treasury_dashboard.json` (45 min)
- [ ] Create `monitoring/grafana_energy_dashboard.json` (45 min)
- [ ] Verify both dashboards show data in Grafana (10 min)
- [ ] Create CI gate job (45 min)
- [ ] Test CI gate locally (15 min)
- [ ] Add operations runbooks to `docs/operations.md` (1 hour)
- [ ] Create `scripts/verify_metrics_coverage.sh` (15 min)
- [ ] Final verification: run MAINNET_READINESS_CHECKLIST (30 min)

**Total Time**: 3.5 hours

---

## Success Criteria

When complete:
- ✅ All 6 treasury metrics emitted and visible in Prometheus
- ✅ All 11 energy metrics emitted and visible in Prometheus
- ✅ Treasury dashboard: 6 panels, all showing live data
- ✅ Energy dashboard: 8 panels, all showing live data
- ✅ CI gate: required check passes on all test strides
- ✅ Runbooks: complete with CLI commands and verification steps
- ✅ Metrics coverage: 100% (no missing AGENTS.md metrics)
- ✅ MAINNET_READINESS_CHECKLIST: all checkboxes marked ✅

---

**Estimated Completion**: 2025-12-19 12:00 EST (2-3 hours from now)
