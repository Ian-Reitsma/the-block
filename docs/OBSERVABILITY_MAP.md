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
contract-cli gov treasury balance
contract-cli gov treasury list --limit 100 | grep -c queued
contract-cli metrics summary | grep "governance_disbursements\|treasury_"
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
contract-cli gov treasury balance | jq .executor

# List stuck disbursements (old created_at)
contract-cli gov treasury list --status queued | jq 'select(.created_at < (now - 3600))'

# Check error metrics
prometheus_query 'rate(treasury_execution_errors_total[5m]) > 0'

# Inspect ledger for failures
contract-cli ledger search --filter 'type=disbursement_failed' | head -20
```

**Resolution**:
1. Check executor logs: `grep treasury_executor /var/log/node/*.log`
2. Verify treasury balance: `contract-cli gov treasury balance`
3. If insufficient funds: Wait for accruals or governance approval
4. If dependency issue: Use `contract-cli gov treasury show --id X` to check dep states
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
contract-cli gov treasury show --id 42

# Verify in ledger
contract-cli ledger search --filter 'disbursement_id=42' | jq 'select(.type=="treasury_transfer")'

# Check receipt
contract-cli receipts search --filter 'source_id=42' --limit 5
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
contract-cli gov treasury balance

# Historical balance changes
contract-cli gov treasury balance --history --limit 50

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
contract-cli energy market --verbose

# Individual provider details
contract-cli energy provider show provider_usa_001

# Pending credits
contract-cli energy credits list --status pending --limit 20

# Metrics summary
contract-cli metrics summary | grep energy_
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
contract-cli energy credits list --status pending | jq '.[] | .expires_at' | sort | uniq -c
```

**Resolution**:
1. Verify oracle node is running: `systemctl status energy-oracle`
2. Check oracle logs: `tail -f /var/log/oracle/*.log`
3. Verify Ed25519 keys: `contract-cli energy oracle keys status`
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
contract-cli energy disputes list --status pending

# Recent slashing
contract-cli energy disputes list --status resolved | grep slashed | tail -10

# Provider reputation impact
contract-cli energy provider show provider_flagged_001 | jq .reputation
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

# Min-payment floor rejections
receipt_min_payment_rejected_total

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
contract-cli receipts stats --market all

# Check each market
contract-cli receipts stats --market storage
contract-cli receipts stats --market compute
contract-cli receipts stats --market energy
contract-cli receipts stats --market ad

# Recent failures
contract-cli receipts search --filter 'status=failed' --limit 20
```

**Verification Checklist**:
- [ ] `receipt_emitted_total` increasing for all 4 markets
- [ ] `receipt_validation_errors_total` < 1% of total receipts
- [ ] `receipt_pending_depth < 1000` per market
- [ ] No market stalled for > 5 blocks

**Runbook**: `docs/operations.md#receipts-flatlining`

---

### Q: Are receipt shards healthy and available?

**Canonical Metrics**:
```promql
# Per-shard load
receipt_shard_count_per_block
receipt_shard_bytes_per_block
receipt_shard_verify_units_per_block

# Availability + integrity
receipt_da_sample_success_total
receipt_da_sample_failure_total
receipt_aggregate_sig_mismatch_total
receipt_header_mismatch_total
receipt_shard_diversity_violation_total
```

**Grafana Dashboard**: `monitoring/grafana_receipt_dashboard.json`
- Panel: "Shard Usage" — Count/bytes/verify-units per shard (filters by shard label)
- Panel: "Receipt DA Samples" — Success vs failure trend
- Panel: "Aggregate Sig Mismatch" — Count of header/signature divergences

**CLI Command**:
```bash
# Spot-check shard load (metrics scrape)
curl -s http://localhost:9000/metrics | grep '^receipt_shard_'

# Inspect latest macro-block receipt roots
contract-cli ledger macro --limit 1 | jq '.receipt_header'
```

**Verification Checklist**:
- [ ] No `receipt_da_sample_failure_total` increase over 10m
- [ ] `receipt_aggregate_sig_mismatch_total` steady at 0
- [ ] Shard usage within configured budgets; no shard spikes relative to peers
- [ ] No `receipt_shard_diversity_violation_total` increments while building/validating blocks

**Runbook**: `docs/operations.md#receipts-flatlining` (add DA drill if sampling fails)

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
contract-cli explorer status

# Check receipt lag
prometheus_query 'explorer_receipt_lag_seconds'

# Verify database
contract-cli explorer db-check --verbose
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
contract-cli governor status | jq '.economics'

# Market metrics
contract-cli economics metrics --market all

# Block reward tracking
contract-cli economics block-reward --limit 100
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
contract-cli ledger verify --sample-size 10000

# Treasury vs ledger
contract-cli ledger search --filter 'type=treasury_transfer' | jq 'map(.amount) | add'
contract-cli gov treasury balance | jq .balance
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
contract-cli gov propose \
  --title "Emergency: Pause Treasury" \
  --param kill_switch_subsidy_reduction=true

# Status
watch -n 10 'contract-cli gov treasury list --status queued | wc -l'
```

### Kill Switch: Halt Energy Settlement

```bash
# Raise slashing rate to prevent bad settlements
contract-cli gov propose \
  --title "Emergency: Energy Slashing (Pause)" \
  --param energy_slashing_rate_bps=10000
```

### Manual Ledger Recovery

```bash
# Inspect corruption
contract-cli ledger dump --range 1000-2000 > ledger_dump.json

# Contact operations team with:
# - ledger_dump.json
# - Error logs (node.log)
# - Prometheus metrics snapshot
```

---

## Extended Domain Maps (Spec Expansion)

The sections below enumerate each subsystem with the same Question → Metrics → Dashboard → Probe → Runbook pattern. Every metric listed exists in `node/src/telemetry/**` or `metrics-aggregator`, dashboards live under `monitoring/grafana_*.json`, and runbooks anchor in `docs/operations.md` unless stated.

### Receipts System

**Key Questions**
- Are receipts emitted and drained every block across all markets?
- Are validation/encoding/decoding errors spiking?
- Is settlement depth staying below alert thresholds?

**Canonical Metrics**
```promql
receipt_bytes_per_block
receipts_storage_per_block
receipt_settlement_storage
receipt_settlement_compute
receipt_settlement_energy
receipt_settlement_ad
receipt_validation_failures_total
receipt_encoding_failures_total
receipt_decoding_failures_total
receipt_drain_operations_total
receipt_persist_fail_total
receipt_corrupt_total
```

**Dashboards**
- `monitoring/grafana_receipt_dashboard.json`
  - Panels: "Receipts per Block", "Settlement by Market", "Validation/Encoding Errors", "Drain Depth"

**Probes / CLI**
```bash
contract-cli receipts stats
contract-cli receipts search --limit 5
curl -s http://localhost:9000/metrics | grep '^receipt_'
```

**Runbooks**
- `docs/operations.md#receipts-flatlining`
- `docs/archive/PHASES_2-4_COMPLETE.md` (rollout + validation checklist)

### Economics & Governance

**Key Questions**
- Are economics gauges aligned with Launch Governor and autopilot?
- Are block rewards, multipliers, and tariffs within allowed bands?
- Are epoch samples flowing (tx count/volume, treasury inflow)?

**Canonical Metrics**
```promql
economics_block_reward_per_block
economics_prev_market_metrics_utilization_ppm
economics_prev_market_metrics_provider_margin_ppm
economics_epoch_tx_count
economics_epoch_tx_volume_block
economics_epoch_treasury_inflow_block
economics_multiplier
economics_subsidy_share_bps
economics_control_law_update_total
economics_tariff_bps
economics_tariff_treasury_contribution_bps
```

**Dashboards**
- Economics row in `monitoring/grafana_treasury_dashboard.json`
- Aggregator `/wrappers` plus `/treasury/summary` and `/energy/summary` overlays

**Probes / CLI**
```bash
tb-cli governor status --rpc <endpoint>
tb-cli governor intents --gate economics --limit 5
contract-cli gov params show | grep economics_
```

**Runbooks**
- `docs/operations.md#telemetry-wiring`
- `docs/architecture.md#energy-governance-and-rpc-next-tasks`

### Networking & QUIC

**Key Questions**
- Are QUIC handshakes succeeding during chaos/TLS rotation?
- Are bytes/retransmits within expected bounds and endpoints being reused?
- Are TLS env warnings surfacing during drills?

**Canonical Metrics**
```promql
quic_handshake_success_total
quic_handshake_fail_total
quic_conn_latency_seconds_bucket
quic_bytes_sent_total
quic_bytes_recv_total
quic_endpoint_reuse_total
transport_provider_connect_total
tls_env_warning_total
tls_env_warning_events_total
```

**Dashboards**
- Network/QUIC row in ops Grafana (shared dashboard)
- TLS warning panels sourced from metrics-aggregator snapshots

**Probes / CLI**
```bash
tb-cli net peer stats --limit 20
tb-cli net tls warnings
./scripts/wan_chaos_drill.sh  # emits chaos + TLS telemetry
```

**Runbooks**
- `docs/MULTI_NODE_CLUSTER_RUNBOOK.md` (chaos/TLS rotation)
- `docs/operations.md#tls-handshake-timeouts`
- `docs/operations.md#p2p-rate-limiting-and-chain-sync`

### Storage & State

**Key Questions**
- Are WAL/snapshot/compaction pipelines healthy?
- Are storage proofs validating without timeouts?
- Are importer/pruner tasks progressing?

**Canonical Metrics**
```promql
storage_engine_pending_compactions
storage_engine_running_compactions
storage_engine_level0_files
storage_engine_sst_bytes
storage_engine_memtable_bytes
storage_engine_size_bytes
storage_proof_validation_seconds
storage_proof_validation_failures_total
simpledb_snapshot_rewrite_total
storage_import_total
storage_prune_total
```

**Dashboards**
- Storage row in `monitoring/grafana_treasury_dashboard.json`
- Import/compaction panels (override JSON if enabled)

**Probes / CLI**
```bash
contract-cli storage stats
contract-cli storage importer status
tb-cli snapshot status
```

**Runbooks**
- `docs/operations.md#storage-and-state`
- `docs/operations.md#snapshots-and-state-pruning`

### Compute Marketplace

**Key Questions**
- Are SLAs meeting deadlines and not piling up?
- Are job timeouts or violations spiking?
- Is lane-aware matching rotating fairly?

**Canonical Metrics**
```promql
compute_job_timeout_total
compute_sla_violations_total
compute_sla_pending_total
compute_sla_next_deadline_ts
compute_sla_automated_slash_total
match_loop_latency_seconds_bucket
receipt_settlement_compute
```

**Dashboards**
- Compute row in generated dashboards:
  - `monitoring/grafana/dashboard.json`
  - `monitoring/grafana/operator.json`
  - `monitoring/grafana/dev.json`
  - `monitoring/grafana/telemetry.json`

**Probes / CLI**
```bash
contract-cli compute jobs list --limit 20
contract-cli compute sla history --limit 20
curl -s http://localhost:9000/metrics | grep '^compute_'
```

**Runbooks**
- `docs/operations.md#compute-marketplace`
- `docs/architecture.md#compute-marketplace`

### Ad Marketplace & Targeting

**Key Questions**
- Are readiness cohorts healthy?
- Are verifier committee rejections increasing?
- Are ad payout receipts flowing?

**Canonical Metrics**
```promql
ad_verifier_committee_rejection_total
ad_readiness_skipped_total
ad_segment_ready_total
ad_market_utilization_observed_ppm
ad_market_utilization_target_ppm
ad_market_utilization_delta_ppm
ad_privacy_budget_remaining_ppm
explorer_block_payout_ad_total
explorer_block_payout_ad_settlement_count
explorer_block_payout_ad_price_usd_micros
```

**Dashboards**
- Ad row in ops Grafana (readiness + payouts)
- Aggregator `/wrappers` readiness snapshots

**Probes / CLI**
```bash
contract-cli ad readiness --cohort all
contract-cli ad payouts --limit 20
```

**Runbooks**
- `docs/architecture.md#ad--targeting-readiness-checklist`
- `docs/operations.md#ad-market` (if present)

### DEX & Bridges

**Key Questions**
- Are trust-line routes and AMM pools liquid?
- Are bridge settlements succeeding without anomaly flags?

**Canonical Metrics**
```promql
dex_liquidity_locked_total
dex_escrow_locked
dex_escrow_pending
dex_orders_total
dex_trade_volume
bridge_settlement_results_total
bridge_liquidity_locked_total
bridge_liquidity_unlocked_total
bridge_liquidity_minted_total
bridge_liquidity_burned_total
bridge_reward_claims_total
bridge_reward_approvals_consumed_total
```

**Dashboards**
- Bridge anomaly/remediation panels in ops Grafana
- DEX liquidity/utilization panels

**Probes / CLI**
```bash
contract-cli dex pools list
contract-cli bridge settlements --limit 20
curl -s http://localhost:9000/metrics | grep 'bridge_'
metrics-aggregator: /anomalies/bridge, /remediation/bridge
```

**Runbooks**
- `docs/operations.md#bridge-and-cross-chain-security`
- `docs/architecture.md#dex-and-trust-lines`

### Metrics Aggregator & Telemetry Stack

**Key Questions**
- Are telemetry ingests and replications healthy?
- Are wrapper snapshots (including /treasury/summary and /energy/summary) exposed?
- Are TLS warnings and chaos attestations flowing?

**Canonical Metrics**
```promql
aggregator_ingest_total
aggregator_telemetry_ingest_total
bulk_export_total
cluster_peer_active_total
aggregator_replication_lag_seconds
tls_env_warning_total
tls_env_warning_events_total
```

**Dashboards**
- Aggregator/TLS rows in ops Grafana
- Chaos status panels

**Probes / HTTP**
```bash
curl -s http://<aggregator>/wrappers | jq .
curl -s http://<aggregator>/treasury/summary | jq .
curl -s http://<aggregator>/energy/summary | jq .
curl -s http://<aggregator>/chaos/status | jq .
curl -s http://<aggregator>/tls/warnings/status | jq .
```

**Runbooks**
- `docs/operations.md#telemetry-wiring`
- `docs/MULTI_NODE_TESTING.md` (multi-node aggregator)

### Alert Pivots & Playbooks

For each alert, capture:
- **Panel link** (Grafana JSON + panel title)
- **PromQL query** (copy/pasteable)
- **CLI/HTTP probe** (tb-cli/contract-cli/aggregator)
- **Runbook anchor** (section in `docs/operations.md`)

Examples:
- Treasury backlog: `treasury_disbursement_backlog` → Grafana "Queue Depth" → `contract-cli gov treasury list --status queued` → `docs/operations.md#treasury-stuck`
- QUIC handshake: `quic_handshake_fail_total` → Grafana "QUIC failures" → `tb-cli net peer stats` → `docs/operations.md#tls-handshake-timeouts`
- Receipts drain: `receipt_drain_operations_total` → Grafana "Receipt drain depth" → `contract-cli receipts stats` → `docs/operations.md#receipts-flatlining`

### Validation Checklist (per deploy)

- [ ] All metrics above return non-empty series in Prometheus
- [ ] Grafana panels show live data post-deploy
- [ ] CLI/HTTP probes succeed (non-5xx)
- [ ] Runbook anchors exist and are current
- [ ] Aggregator `/wrappers`, `/treasury/summary`, `/energy/summary` respond 200 with expected fields

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
