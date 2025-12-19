# Operations Runbooks: Fast-Mainnet

**Purpose**: Step-by-step troubleshooting guides for operational issues  
**Audience**: Site Reliability Engineers, Launch Operations Team  
**Related**: `docs/OBSERVABILITY_MAP.md` (mapping questions to metrics)

---

## Treasury Stuck

### Symptoms

- [ ] `treasury_disbursement_backlog > 50` for 2+ epochs
- [ ] `treasury_disbursement_lag_seconds_p95 > 300`
- [ ] Executor reports `last_error != null`
- [ ] Stale disbursements (created_at > 3 days ago, still Queued)
- [ ] No state transitions occurring

### Diagnosis

**Step 1**: Check executor health

```bash
contract-cli gov treasury balance | jq .executor
# Look for:
#   last_error: string (should be null)
#   last_success_at: recent timestamp
#   pending_matured: count of ready-to-execute
#   lease_holder: node_a (should be active node)
#   lease_expires_at: future timestamp
```

**Step 2**: List stuck disbursements

```bash
echo "=== Disbursements stuck for >1 hour ==="
contract-cli gov treasury list --limit 200 --status queued | jq \
  '.disbursements[] | select(.created_at < (now - 3600)) | {id, created_at, updated_at}'

echo "=== Count stuck disbursements ==="
contract-cli gov treasury list --limit 200 --status queued | jq \
  '.disbursements | length'
```

**Step 3**: Check for dependency failures

```bash
echo "=== Queued disbursements with dependencies ==="
contract-cli gov treasury list --status queued --limit 50 | while read id; do
  deps=$(contract-cli gov treasury show --id "$id" | jq '.proposal.deps')
  if [ "$deps" != "[]" ]; then
    echo "ID $id depends on: $deps"
    # Check if dependencies are satisfied
    echo "$deps" | jq '.[]' | while read dep_id; do
      state=$(contract-cli gov treasury show --id "$dep_id" | jq -r '.status.state')
      echo "  ↳ Dependency $dep_id: $state"
    done
  fi
done
```

**Step 4**: Inspect error logs

```bash
echo "=== Recent treasury executor errors ==="
grep "treasury_executor\|DisbursementError" /var/log/node/*.log \
  | grep -E "ERROR|WARN" \
  | tail -50

echo "=== Executor tick duration ==="
prometheus_query 'histogram_quantile(0.99, treasury_executor_tick_duration_seconds_bucket)'

echo "=== Execution error rate ==="
prometheus_query 'rate(treasury_execution_errors_total[5m])'
```

**Step 5**: Check treasury balance

```bash
contract-cli gov treasury balance | jq '{balance_ct, balance_it}'

# If insufficient, show what's waiting to execute
echo "=== Pending disbursements (sum of amounts) ==="
contract-cli gov treasury list --status queued --limit 200 | jq \
  '.disbursements | map(.amount_ct) | add'
```

### Resolution Paths

#### **Path A: Dependency Issue**

**Indicators**: Dependencies in wrong state

```bash
# List dependencies and their states
contract-cli gov treasury show --id <STUCK_ID> | jq '{id: .id, deps: .proposal.deps}'

# For each dependency, check state
for dep_id in $(contract-cli gov treasury show --id <STUCK_ID> | jq -r '.proposal.deps[]'); do
  state=$(contract-cli gov treasury show --id "$dep_id" | jq -r '.status.state')
  echo "Dependency $dep_id: $state"
  
  if [ "$state" = "rolled_back" ]; then
    echo "  ✗ Dependency failed. Cancelling dependent..."
    # Cancel the stuck disbursement
    contract-cli gov treasury rollback --id <STUCK_ID> \
      --reason "Dependency $dep_id was rolled back"
  fi
done
```

#### **Path B: Insufficient Funds**

**Indicators**: `treasury_execution_errors_total{reason="insufficient_funds"}` increasing

```bash
# Check current balance
current=$(contract-cli gov treasury balance | jq .balance_ct)
pending=$(contract-cli gov treasury list --status queued | jq '[.disbursements[].amount_ct] | add')

echo "Current balance: $current CT"
echo "Pending disbursements: $pending CT"

if [ $current -lt $pending ]; then
  echo "INSUFFICIENT FUNDS"
  echo "Wait for accruals: watch -n 30 'contract-cli gov treasury balance | jq .balance_ct'"
  echo "OR request governance approval for fund allocation"
fi
```

#### **Path C: Executor Error**

**Indicators**: `last_error != null`, executor not progressing

```bash
# Get the exact error
error=$(contract-cli gov treasury balance | jq -r '.executor.last_error')
echo "Executor error: $error"

# Common errors and fixes:
case "$error" in
  "ledger_unavailable")
    echo "Ledger connection lost. Check: systemctl status ledger-node"
    ;;
  "consensus_timeout")
    echo "Consensus stalled. Check: contract-cli node consensus --status"
    ;;
  "dependency_cycle_detected")
    echo "Circular dependency found. Review recent disbursement submissions."
    ;;
  *)
    echo "Unknown error. Check logs: tail -100 /var/log/node/*.log"
    ;;
esac

# Restart executor
echo "Attempting executor restart..."
sudo systemctl restart the-block

# Monitor recovery
echo "Monitoring backlog recovery..."
watch -n 5 'contract-cli gov treasury balance | jq .executor.last_error'
```

#### **Path D: Data Corruption**

**Indicators**: Multiple disbursements stuck with inconsistent states

```bash
# Snapshot current state
contract-cli gov treasury list --limit 500 > /tmp/disburse_snapshot_$(date +%s).json
grep "treasury_executor" /var/log/node/*.log > /tmp/executor_logs_$(date +%s).txt

# Contact operations with:
echo "Send to ops team:"
echo "  - /tmp/disburse_snapshot_*.json"
echo "  - /tmp/executor_logs_*.txt"
echo "  - prometheus_dump.json (from curl http://localhost:9090/api/v1/query_range?...)"
echo "  - Describe when issue started"
```

### Alert Thresholds

**CRITICAL** (Page on-call):
```promql
# Backlog accumulating
treasury_disbursement_backlog > 100 for 3 epochs

# Executor failing
rate(treasury_execution_errors_total[5m]) > 1

# No progress for extended period
increase(governance_disbursements_total{status="finalized"}[10m]) == 0
```

**WARNING** (Create ticket):
```promql
# High latency
histogram_quantile(0.95, treasury_disbursement_lag_seconds_bucket) > 600

# Moderate backlog
treasury_disbursement_backlog > 50 for 2 epochs
```

---

## Energy Stalled

### Symptoms

- [ ] `oracle_latency_seconds_p95 > 10`
- [ ] `energy_signature_verification_failures_total` increasing rapidly (> 1/min)
- [ ] `energy_pending_credits_total` not decreasing
- [ ] Provider readings not settling
- [ ] Disputes backlog increasing

### Diagnosis

**Step 1**: Check oracle latency

```bash
echo "=== Oracle latency distribution ==="
prometheus_query 'histogram_quantile(0.50, oracle_latency_seconds_bucket)'
prometheus_query 'histogram_quantile(0.95, oracle_latency_seconds_bucket)'
prometheus_query 'histogram_quantile(0.99, oracle_latency_seconds_bucket)'

echo "=== Oracle process status ==="
sudo systemctl status energy-oracle
grep oracle_latency /var/log/oracle/*.log | tail -20
```

**Step 2**: Check signature verification failures

```bash
echo "=== Signature failure rate ==="
prometheus_query 'rate(energy_signature_verification_failures_total[5m])'

echo "=== Recent verification failures ==="
grep "signature_verification_failed\|SignatureInvalid" /var/log/node/*.log | tail -30

echo "=== Breakdown by reason ==="
for reason in invalid_format verification_failed key_not_found scheme_unsupported; do
  count=$(prometheus_query "energy_signature_verification_failures_total{reason=\"$reason\"}" | jq '.data.result[0].value[1]')
  echo "  $reason: $count"
done
```

**Step 3**: Check meter reading accumulation

```bash
echo "=== Pending credits (kWh) ==="
contract-cli energy credits list --status pending --limit 50 | jq \
  '.credits | {total_kwh: map(.amount_kwh) | add, count: length}'

echo "=== Credits by provider ==="
contract-cli energy credits list --status pending --limit 200 | jq \
  '.credits | group_by(.provider_id) | map({provider: .[0].provider_id, count: length, total_kwh: map(.amount_kwh) | add})'
```

**Step 4**: Check for timestamp issues

```bash
echo "=== Timestamp skew errors ==="
grep "timestamp_skew\|TimestampSkew" /var/log/node/*.log | wc -l

echo "=== System clock status ==="
timedatectl
ntpq -p

echo "=== Provider clock differences ==="
for provider in $(contract-cli energy market | jq -r '.providers[].provider_id' | head -10); do
  last_reading=$(contract-cli energy provider show "$provider" | jq .last_settlement)
  echo "$provider: $last_reading"
done
```

**Step 5**: Check provider status

```bash
echo "=== Inactive providers ==="
contract-cli energy market | jq '.providers[] | select(.status != "active") | {provider_id, status, reputation_score}'

echo "=== Providers with poor reputation ==="
contract-cli energy market | jq '.providers[] | select(.reputation.composite_score < 0.5) | {provider_id, score: .reputation.composite_score}'
```

### Resolution Paths

#### **Path A: Oracle Latency High**

**Indicators**: p95 latency > 10 seconds

```bash
# Check oracle CPU and memory
ps aux | grep oracle
top -p $(pgrep -f oracle)

# Check oracle queue depth
grep "pending_verifications\|queue_depth" /var/log/oracle/*.log | tail -10

# Scale oracle if needed (multiple instances)
echo "Consider: kubectl scale deployment energy-oracle --replicas=3"

# Monitor improvement
watch -n 5 'prometheus_query "histogram_quantile(0.95, oracle_latency_seconds_bucket)"'
```

#### **Path B: Signature Verification Failures**

**Indicators**: Failures > 1/min, reason = "verification_failed" or "invalid_format"

```bash
# Check provider keys
echo "=== Checking oracle key manager ==="
contract-cli energy oracle keys status

# Verify provider public keys in registry
for provider in $(contract-cli energy market | jq -r '.providers[].provider_id' | head -5); do
  key=$(contract-cli energy provider show "$provider" | jq .public_key)
  echo "$provider: $key"
done

# Test a signature manually
echo "=== Testing signature generation ==="
cat > /tmp/test_sig.py << 'EOF'
import ed25519, struct, base64, time

provider_id = "provider_usa_001"
meter = "meter_001"
total_kwh = 1500000
timestamp = int(time.time())
nonce = 12345

# Build message
message = (
    provider_id.encode() +
    meter.encode() +
    struct.pack('<Q', total_kwh) +
    struct.pack('<Q', timestamp) +
    struct.pack('<Q', nonce)
)

# Test signature
signing_key = ed25519.SigningKey(base64.b64decode("<PRIVATE_KEY>"))
signature = signing_key.sign(message).signature
print(f"Signature: {base64.b64encode(signature)}")
EOF
python /tmp/test_sig.py
```

#### **Path C: Timestamp Skew**

**Indicators**: "timestamp_skew" errors in logs

```bash
# Check system clock on provider and oracle
echo "=== Provider nodes ==="
for node in provider_node_1 provider_node_2 oracle_node; do
  echo "$node:"
  ssh "$node" 'date; timedatectl'
done

# Fix time drift
echo "Syncing clocks with NTP..."
sudo ntpdate -u ntp.ubuntu.com  # or your NTP server

# Verify sync
timedatectl
for node in provider_node_1 provider_node_2; do
  ssh "$node" 'timedatectl'
done

# Monitor recovery
watch -n 10 'grep timestamp_skew /var/log/node/*.log | tail -5'
```

#### **Path D: Provider Reputation Degradation**

**Indicators**: Multiple providers with score < 0.5

```bash
# Check what caused reputation drops
echo "=== Recent disputes ==="
contract-cli energy disputes list --limit 20

# Check slashing events
echo "=== Recent slashing ==="
prometheus_query 'rate(energy_slashing_total[24h])' | jq '.data.result[] | {provider, reason: .metric.reason, rate: .value}'

# Review evidence
for dispute_id in $(contract-cli energy disputes list --status resolved | jq -r '.disputes[].dispute_id' | head -5); do
  echo "Dispute $dispute_id:"
  contract-cli energy disputes show --id "$dispute_id"
done
```

### Alert Thresholds

**CRITICAL** (Page on-call):
```promql
# Oracle broken
oracle_latency_seconds_p95 > 30
energy_signature_verification_failures_total > 10

# Settlement stalled
increase(energy_settlements_total[10m]) == 0

# Dispute backlog critical
energy_active_disputes_total > 50
```

**WARNING** (Create ticket):
```promql
oracle_latency_seconds_p95 > 10
rate(energy_signature_verification_failures_total[5m]) > 1
energy_active_disputes_total > 20
```

---

## Receipts Flatlining

### Symptoms

- [ ] `receipt_emitted_total` flat or decreasing for 5+ blocks
- [ ] One or more markets (storage, compute, energy, ad) not emitting
- [ ] `receipt_validation_errors_total` increasing
- [ ] Explorer receipt tables not updating

### Diagnosis

**Step 1**: Check emission rates by market

```bash
echo "=== Receipt emission rate (1m) ==="
prometheus_query 'rate(receipt_emitted_total[1m])' | jq '.data.result[] | {market: .metric.market, rate: .value[1]}'

echo "=== Markets with no emissions (last 10 blocks) ==="
prometheus_query 'increase(receipt_emitted_total[10m]) == 0' | jq '.data.result[].metric'
```

**Step 2**: Check validation errors

```bash
echo "=== Validation error rate ==="
prometheus_query 'rate(receipt_validation_errors_total[5m])'

echo "=== Error breakdown by reason ==="
for reason in schema_mismatch duplicate_detection signature_invalid; do
  count=$(prometheus_query "receipt_validation_errors_total{reason=\"$reason\"}" | jq '.data.result[0].value[1]')
  echo "  $reason: $count"
done
```

**Step 3**: Per-market diagnostics

```bash
# Storage market
echo "=== Storage market ==="
contract-cli receipts stats --market storage
grep storage /var/log/node/receipts.log | tail -20

# Compute market
echo "=== Compute market ==="
contract-cli receipts stats --market compute
grep compute /var/log/node/receipts.log | tail -20

# Energy market
echo "=== Energy market ==="
contract-cli receipts stats --market energy
grep energy /var/log/node/receipts.log | tail -20

# Ad market
echo "=== Ad market ==="
contract-cli receipts stats --market ad
grep ad /var/log/node/receipts.log | tail -20
```

**Step 4**: Check block height and progression

```bash
echo "=== Current block ==="
contract-cli node status | jq '.current_block_height'

echo "=== Block progression (last 50 blocks) ==="
prometheus_query 'increase(block_height_total[50m])'

echo "=== Consensus status ==="
contract-cli node consensus --status
```

### Resolution

**For stalled markets**:

```bash
# Restart receipt emitter
sudo systemctl restart receipt-emitter

# Monitor recovery
watch -n 5 'contract-cli receipts stats --market storage'

# If persists, check market-specific service
for market in storage compute energy ad; do
  sudo systemctl status "${market}-market"
done
```

**For validation errors**:

```bash
# Clear validation state (if safe)
contract-cli receipts reset-validation-state

# Re-validate last N blocks
contract-cli receipts validate --from-block $((
  $(contract-cli node status | jq .current_block_height) - 100
))

# Monitor
watch -n 10 'prometheus_query "receipt_validation_errors_total"'
```

---

## Settlement Audit

### How to Run

```bash
# Standard settlement audit
cargo test -p the_block --test settlement_audit --release -- --nocapture

# With specific options
STARTING_EPOCH=0 ENDING_EPOCH=1000 cargo test \
  -p the_block --test settlement_audit --release -- --nocapture

# Verbose output
RUST_LOG=debug cargo test -p the_block --test settlement_audit --release -- --nocapture
```

### Interpreting Results

**Successful audit**:
```
test settlement_audit ... ok

Ledger conservation verified:
  Initial balance: 10,000,000 CT
  Accruals: 1,500,000 CT
  Executed disbursements: 2,000,000 CT
  Final balance: 9,500,000 CT
```

**Failed audit** (example):
```
test settlement_audit ... FAILED

Assertion failed:
  Expected balance: 9,500,000 CT
  Actual balance: 9,300,000 CT
  Discrepancy: 200,000 CT (2.1%)

Investigation:
  1. Find missing disbursement: ID 4521
  2. Check status: Executed but not credited
  3. Verify: Receipt exists? Yes. Target account? Valid.
  4. Root cause: Ledger index out of sync
```

### Troubleshooting

**If audit fails**:

```bash
# Get detailed logs
cargo test -p the_block --test settlement_audit --release -- --nocapture --test-threads=1 2>&1 | tee settlement_audit.log

# Extract specific disbursement details
grep "Disbursement 4521" settlement_audit.log

# Check ledger state
contract-cli node ledger inspect --account treasury_account_id

# Verify receipts associated with missing disbursement
grep "disbursement_id.*4521" /var/log/node/*.log
```

---

## Helper Functions

### prometheus_query()

**Purpose**: Query Prometheus for metric values

**Usage**:
```bash
prometheus_query 'up{instance="localhost:9090"}'
prometheus_query 'histogram_quantile(0.95, request_duration_seconds_bucket)'
```

**Implementation**:
```bash
prometheus_query() {
  local query="$1"
  local url="${PROMETHEUS_URL:-http://localhost:9090}"
  curl -s "${url}/api/v1/query" \
    --data-urlencode "query=${query}" | \
    jq -r '.data.result[0].value[1] // "no data"'
}
```

**Configuration**:
```bash
export PROMETHEUS_URL="http://prometheus.infra.internal:9090"
```

---

## SLO Definitions

### Treasury System

| Metric | Target | Alert Threshold |
|--------|--------|----------|
| Availability | 99.95% | Errors > 0.1% for 5 min |
| Execution Latency p95 | < 300s | > 600s for 10 min |
| Error Rate | < 0.1% | > 1 error/sec |
| Queue Depth | < 100 | > 100 for 3 epochs |

### Energy System

| Metric | Target | Alert Threshold |
|--------|--------|----------|
| Oracle Latency p95 | < 5s | > 10s for 5 min |
| Signature Validation | > 99.9% | > 1 failure/min |
| Settlement Rate | > 95% | < 90% for 10 min |
| Dispute Resolution | < 1 hour | Unresolved > 2 hours |

### Receipts System

| Metric | Target | Alert Threshold |
|--------|--------|----------|
| Emission Rate | All markets | Any market = 0 for 5 blocks |
| Validation Success | > 99.99% | < 99% for 10 min |
| Storage | All receipts | Storage > 1 week |
| Query Latency p99 | < 100ms | > 500ms for 5 min |

---

**Last Updated**: 2025-12-19  
**Next Review**: 2025-12-26  
**Maintainer**: Operations Team  
