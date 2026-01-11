# Fast-Mainnet Observability Map

**Purpose**: Single source of truth mapping operational questions to metrics, dashboards, and runbooks  
**Last Updated**: 2025-12-19  
**Status**: Complete for Treasury, Energy, Receipts, and Economics

---

## Directory Structure

This document provides complete traceability for mainnet readiness:

```
Question (What operators ask)
├── Canonical Metrics (Which Prometheus series to query)
├── Grafana Dashboard (Which panels visualize this)
├── CLI Command (Real-time state inspection)
└── Runbook (Step-by-step troubleshooting)
```

---

## Treasury System

### Q: Is the treasury system healthy?

**Canonical Metrics**:
```promql
# Current status breakdown
gov_ernance_disbursements_total{status=~"queued|timelocked|executed|finalized|rolled_back"}

# Backlog by state
treasury_disbursement_backlog{status=~".*"}

# Execution health
treasury_execution_errors_total{reason=~".*"}

# Balance snapshot
treasury_balance
```

**Grafana Dashboard**: `monitoring/grafana_treasury_dashboard.json`
- Panel: "Disbursement Pipeline" — Shows state transitions over time
- Panel: "Queue Depth by Status" — Current backlog distribution
- Panel: "Execution Errors" — Error types and frequency
- Panel: "Balance Trend" — Treasury BLOCK balance over time

**CLI Command**:
```bash
tb-cli gov treasury balance
tb-cli gov treasury list --limit 100 | grep -c queued
tb-cli metrics summary | grep "governance_disbursements\|treasury_"
```

**Runbook**: `docs/operations.md#treasury-stuck`

---

### Q: Are disbursements stuck in a particular state?

**Symptoms**:
- Backlog metric stays > 50 for more than 2 epochs
- `treasury_disbursement_lag_seconds` p95 > 300 seconds
- Executor reports `last_error`

**Diagnosis**:
```bash
# Check executor health
tb-cli gov treasury balance | jq .executor

# List stuck disbursements (old created_at)
tb-cli gov treasury list --status queued | jq 'select(.created_at < (now - 3600))'

# Check error metrics
prometheus_query 'rate(treasury_execution_errors_total[5m]) > 0'

# Inspect ledger for failures
tb-cli ledger search --filter 'type=disbursement_failed' | head -20
```

**Resolution**:
1. Check executor logs: `grep treasury_executor /var/log/node/*.log`
2. Verify treasury balance: `tb-cli gov treasury balance`
3. If insufficient funds: Wait for accruals or governance approval
4. If dependency issue: Use `tb-cli gov treasury show --id X` to check dep states
5. If data corruption: Contact ops (manual ledger recovery)

**Alert Threshold**:
- CRITICAL: `treasury_disbursement_backlog > 100` for 3+ epochs
- WARNING: `treasury_disbursement_lag_seconds_p95 > 600`

---

### Q: Did a disbursement execute correctly?

**Canonical Metrics**:
```promql
gov_ernance_disbursements_total{status="finalized"}
treasury_disbursement_lag_seconds  # Histogram percentiles
```

**Grafana Dashboard**: `monitoring/grafana_treasury_dashboard.json`
- Panel: "Finalized Count (24h)" — Successful executions
- Panel: "Lag Percentiles" — Execution duration distribution

**CLI Command**:
```bash
# Get disbursement details
tb-cli gov treasury show --id 42

# Verify in ledger
tb-cli ledger search --filter 'disbursement_id=42' | jq 'select(.type=="treasury_transfer")'

# Check receipt
tb-cli receipts search --filter 'source_id=42' --limit 5
```

**Verification**:
- [ ] Status shows `finalized`
- [ ] `executed_at` timestamp is reasonable
- [ ] `tx_hash` matches ledger entry
- [ ] Destination account received BLOCK
- [ ] Settlement audit passes (see Stride 3)

---

### Q: Is the treasury balance accurate?

**Canonical Metrics**:
`treasury_balance`

**CLI Command**:
```bash
# Current balance
tb-cli gov treasury balance

# Historical balance changes
tb-cli gov treasury balance --history --limit 50

# Settlement audit
cargo test -p the_block --test settlement_audit --release -- --nocapture
```

**Verification**:
```
# Ledger conservation check:
treasury_initial + accruals - executed = treasury_current + [pending in pipeline]
```

**Runbook**: `docs/operations.md#settlement-audit`

---

## Energy System

### Q: Are energy providers operating normally?

**Canonical Metrics**:
```promql
# Provider inventory
energy_provider_total
energy_provider_status{status="active"}

# Settlement activity
energy_settlements_total{provider=~".*"}
energy_settlement_total

# Oracle health
oracle_latency_seconds
energy_signature_verification_failures_total
```

**Grafana Dashboard**: `monitoring/grafana_energy_dashboard.json`
- Panel: "Active Providers" — Provider count trend
- Panel: "Settlements (24h)" — Settlement rate per provider
- Panel: "Oracle Latency" — Verification latency distribution
- Panel: "Reputation Scores" — Provider scores over time

**CLI Command**:
```bash
# Provider inventory
tb-cli energy market --verbose

# Individual provider details
tb-cli energy provider show provider_usa_001

# Pending credits
tb-cli energy credits list --status pending --limit 20

# Metrics summary
tb-cli metrics summary | grep energy_
```

**Runbook**: `docs/operations.md#energy-stalled`

---

### Q: Is oracle verification working?

**Symptoms**:
- `oracle_latency_seconds_p95 > 10`
- `energy_signature_verification_failures_total` increasing
- `energy_pending_credits_total` not decreasing

**Diagnosis**:
```bash
# Check oracle latency
prometheus_query 'oracle_latency_seconds{quantile="0.95"}'

# List recent verification failures
grep "signature_verification" /var/log/node/energy.log | tail -20

# Check for timestamp skew
grep "timestamp_skew" /var/log/node/energy.log | wc -l

# Pending credits accumulation
tb-cli energy credits list --status pending | jq '.[] | .expires_at' | sort | uniq -c
```

**Resolution**:
1. Verify oracle node is running: `systemctl status energy-oracle`
2. Check oracle logs: `tail -f /var/log/oracle/*.log`
3. Verify Ed25519 keys: `tb-cli energy oracle keys status`
4. If timestamp skew: Check provider and oracle clock sync (`ntpd`)
5. If signature failures: Verify provider public keys in registry

**Alert Threshold**:
- CRITICAL: `oracle_latency_seconds_p95 > 30` OR `energy_signature_verification_failures_total > 10/min`
- WARNING: `oracle_latency_seconds_p95 > 10`

---

### Q: Are disputes being resolved?

**Canonical Metrics**:
```promql
energy_active_disputes_total
energy_dispute_outcomes_total{outcome=~"resolved|slashed|dismissed"}
energy_disputes_resolved_total{outcome="slashed"}
```

**Grafana Dashboard**: `monitoring/grafana_energy_dashboard.json`
- Panel: "Active Disputes" — Current backlog
- Panel: "Dispute Outcomes" — Resolution rates
- Panel: "Slashing Events" — Provider penalties

**CLI Command**:
```bash
# Active disputes
tb-cli energy disputes list --status pending

# Recent slashing
tb-cli energy disputes list --status resolved | grep slashed | tail -10

# Provider reputation impact
tb-cli energy provider show provider_flagged_001 | jq .reputation
```

**Runbook**: `docs/operations.md#energy-dispute-resolution`

---

## Receipts System

### Q: Are all markets emitting receipts?

**Canonical Metrics**:
```promql
# Receipt emission by market
receipt_emitted_total{market=~"storage|compute|energy|ad"}

# Validation failures
receipt_validation_errors_total{reason=~".*"}

# Pending depth
receipt_pending_depth{market=~".*"}
```

**Grafana Dashboard**: `monitoring/grafana_receipt_dashboard.json`
- Panel: "Receipt Rate by Market" — Emission rate per market
- Panel: "Validation Errors" — Error types and frequency
- Panel: "Pending Receipts" — Backlog by market

**CLI Command**:
```bash
# Receipt statistics
tb-cli receipts stats --market all

# Check each market
tb-cli receipts stats --market storage
tb-cli receipts stats --market compute
tb-cli receipts stats --market energy
tb-cli receipts stats --market ad

# Recent failures
tb-cli receipts search --filter 'status=failed' --limit 20
```

**Verification Checklist**:
- [ ] `receipt_emitted_total` increasing for all 4 markets
- [ ] `receipt_validation_errors_total` < 1% of total receipts
- [ ] `receipt_pending_depth < 1000` per market
- [ ] No market stalled for > 5 blocks

**Runbook**: `docs/operations.md#receipts-flatlining`

---

### Q: Are receipts flowing to explorers?

**Canonical Metrics**:
```promql
# Explorer ingestion
explorer_receipt_processed_total
explorer_receipt_lag_seconds

# Database synchronization
explorer_db_sync_lag_blocks
```

**CLI Command**:
```bash
# Explorer status
tb-cli explorer status

# Check receipt lag
prometheus_query 'explorer_receipt_lag_seconds'

# Verify database
tb-cli explorer db-check --verbose
```

---

## Economics System

### Q: Is the economic system stable?

**Canonical Metrics**:
```promql
# Epoch-level economics
economics_epoch_tx_count
economics_epoch_tx_volume_block
economics_epoch_treasury_inflow_block

# Market metrics
economics_prev_market_metrics_utilization_ppm{market=~".*"}
economics_prev_market_metrics_provider_margin_ppm{market=~".*"}

# Issuance
economics_block_reward_per_block
```

**Grafana Dashboard**: `monitoring/grafana_economics_dashboard.json`
- Panel: "Epoch Throughput" — TX count and volume
- Panel: "Market Utilization" — Usage across markets
- Panel: "Block Reward" — Minting rate over time
- Panel: "Treasury Inflow" — Fee accrual rate

**CLI Command**:
```bash
# Economics snapshot
tb-cli governor status | jq '.economics'

# Market metrics
tb-cli economics metrics --market all

# Block reward tracking
tb-cli economics block-reward --limit 100
```

**Health Checks**:
- [ ] `economics_epoch_tx_count` stable week-over-week
- [ ] `economics_prev_market_metrics_*` variance < 5%
- [ ] `economics_block_reward_per_block` within governance targets

---

## Cross-System Verification

### Ledger Conservation (The – Block Constraint)

**Canonical Metrics**:
```promql
# Settlement audit results
settlement_audit_balance_total  # Should equal blockchain
settlement_audit_conservation_failures_total
```

**Verification Command**:
```bash
# Full settlement audit
cargo test -p the_block --test settlement_audit --release -- --nocapture

# Quick ledger check
tb-cli ledger verify --sample-size 10000

# Treasury vs ledger
tb-cli ledger search --filter 'type=treasury_transfer' | jq 'map(.amount) | add'
tb-cli gov treasury balance | jq .balance
```

**Alert**: If these differ: STOP and investigate (data corruption)

---

## Mainnet Readiness Checklist

### Treasury ✅

- [ ] `treasury_disbursement_backlog < 50` consistently
- [ ] `treasury_execution_errors_total` < 1 per hour
- [ ] `treasury_disbursement_lag_seconds_p95 < 300s`
- [ ] Executor `last_error == null`
- [ ] No stale disbursements (> 3 days in Queued)
- [ ] Settlement audit passes with treasury transfers
- [ ] Explorer treasury timeline synced with RPC

### Energy ✅

- [ ] `energy_provider_total > 100` (sufficient ecosystem)
- [ ] `oracle_latency_seconds_p95 < 10`
- [ ] `energy_signature_verification_failures_total < 1/min`
- [ ] `energy_active_disputes_total < 10` (not backlog)
- [ ] Dispute resolution SLO < 3 epochs
- [ ] All provider reputation scores > 0.5 (confident)
- [ ] `energy_slashing_total` only for clear violations

### Receipts ✅

- [ ] All 4 markets emitting receipts every block
- [ ] `receipt_validation_errors_total < 0.1%` of emission rate
- [ ] `receipt_pending_depth < 1000` per market
- [ ] No market stalled for > 5 blocks
- [ ] Explorer receipt tables updated live
- [ ] Receipt replay tests passing

### Economics ✅

- [ ] `economics_epoch_tx_count` stable (no crashes)
- [ ] Market metrics variance < 5% epoch-to-epoch
- [ ] Block reward tracking correctly (Launch Governor aligned)
- [ ] `economics_block_reward_per_block` within [±5% of target]
- [ ] Treasury inflow metrics matching ledger fees

### Settlement Audit ✅

- [ ] Ledger conservation: BLOCK supply stable
- [ ] No rounding errors in fee distribution
- [ ] Disbursement entries match treasury state
- [ ] Receipt deduplication working (no double-settlement)
- [ ] Replay test passing (deterministic)

---

## Emergency Operations

### Kill Switch: Halt Treasury

```bash
# Governance proposal to pause disbursements
tb-cli gov propose \
  --title "Emergency: Pause Treasury" \
  --param kill_switch_subsidy_reduction=true

# Status
watch -n 10 'tb-cli gov treasury list --status queued | wc -l'
```

### Kill Switch: Halt Energy Settlement

```bash
# Raise slashing rate to prevent bad settlements
tb-cli gov propose \
  --title "Emergency: Energy Slashing (Pause)" \
  --param energy_slashing_rate_bps=10000
```

### Manual Ledger Recovery

```bash
# Inspect corruption
tb-cli ledger dump --range 1000-2000 > ledger_dump.json

# Contact operations team with:
# - ledger_dump.json
# - Error logs (node.log)
# - Prometheus metrics snapshot
```

---

## Metric Coverage Verification

Script to verify all AGENTS.md metrics are emitted:

```bash
#!/bin/bash
METRICS=(
  "governance_disbursements_total"
  "treasury_disbursement_backlog"
  "treasury_disbursement_lag_seconds"
  "treasury_execution_errors_total"
  "treasury_balance"
  "energy_provider_total"
  "energy_pending_credits_total"
  "energy_active_disputes_total"
  "oracle_latency_seconds"
  "energy_slashing_total"
  "receipt_emitted_total"
  "receipt_validation_errors_total"
  "economics_epoch_tx_count"
  "economics_block_reward_per_block"
)

for metric in "${METRICS[@]}"; do
  count=$(curl -s 'http://localhost:9090/api/v1/query' --data-urlencode "query=$metric" | jq '.data.result | length')
  if [ "$count" -eq 0 ]; then
    echo "MISSING: $metric"
  else
    echo "OK: $metric ($count series)"
  fi
done
```

---

## Dashboard Maintenance

When updating code that affects metrics:

1. Update metric definition in `node/src/telemetry/*.rs`
2. Update dashboard in `monitoring/grafana_*.json` (same panel name)
3. Update this observability map with new query
4. Run verification script above
5. Test in staging: `make monitor-test`

---

For complete runbooks, see: `docs/operations.md`
