# Operations Runbooks: Fast-Mainnet

**Purpose**: Step-by-step troubleshooting guides for operational issues  
**Audience**: Site Reliability Engineers, Launch Operations Team  
**Related**: `docs/OBSERVABILITY_MAP.md` (mapping questions to metrics)

---

## Telemetry Wiring

### Runtime Reactor

- `runtime_read_without_ready_total` increments when reads succeed without a readiness event (missed IO wakeups). Sustained growth indicates reactor/event-mapping issues.
- `runtime_write_without_ready_total` increments when writes succeed without a readiness event (missed IO wakeups). Sustained growth indicates reactor/event-mapping issues.

### Runtime Tuning Knobs

- `runtime.reactor_idle_poll_ms`: maximum sleep between reactor polls (ms).
- `runtime.io_read_backoff_ms`: fallback delay before retrying reads when readiness is missing (ms).
- `runtime.io_write_backoff_ms`: fallback delay before retrying writes when readiness is missing (ms).
- **Defaults:** `reactor_idle_poll_ms=100`, `io_read_backoff_ms=10`, `io_write_backoff_ms=10`.
- **When to tune:** Lower the idle poll and backoff values for latency-sensitive testnets; raise them when CPU is saturated by idle peers. Watch `runtime_read_without_ready_total` / `runtime_write_without_ready_total` and reactor CPU to validate changes.
- **BSD note:** On kqueue platforms we run level-triggered mode (no `EV_CLEAR`); if wakeups are missed, increase the backoff and ensure `update_interest()` instrumentation is present.
- **Env vars:** `TB_REACTOR_IDLE_POLL_MS`, `TB_IO_READ_BACKOFF_MS`, and `TB_IO_WRITE_BACKOFF_MS` map to the same knobs; config reloads apply without restart.

### P2P Rate Limiting and Chain Sync

| Knob | Default | Description |
|------|---------|-------------|
| `p2p_rate_window_secs` (`TB_P2P_RATE_WINDOW_SECS`) | 1 | Sliding window for request counters |
| `p2p_max_per_sec` (`TB_P2P_MAX_PER_SEC`) | workload-dependent | Max requests per peer per window |
| `p2p_max_bytes_per_sec` (`TB_P2P_MAX_BYTES_PER_SEC`) | workload-dependent | Max bytes per peer per window |
| `p2p_chain_sync_interval_ms` (`TB_P2P_CHAIN_SYNC_INTERVAL_MS`) | 500 | Periodic chain sync pull interval |

- Use narrow windows and lower maxima during incident response; widen for WAN drills. Keep `p2p_chain_sync_interval_ms` at 0 in isolation tests to prevent periodic pulls.

### Config Hot-Reload Fallback

- Config watcher prefers inotify/kqueue; when unavailable, it falls back to mtime polling. On platforms without reliable fs events, expect up to one poll interval of delay before reload. Documented in `node/src/config.rs`; operators can reduce the poll interval for faster propagation.

### TLS Handshake Timeouts

- HTTP servers use `ServerConfig.tls_handshake_timeout` for TLS handshakes.
- HTTP clients use `ClientConfig.tls_handshake_timeout` or `TlsConnectorBuilder::handshake_timeout`.
- Environment override: `TB_TLS_HANDSHAKE_TIMEOUT_MS` (milliseconds).

### Economics Autopilot Gate

- **Telemetry sources**
  - `economics_block_reward_per_block` shows the current base reward that Launch Governor was replaying when it evaluated economics.
  - `economics_prev_market_metrics_{utilization,provider_margin}_ppm` mirror the deterministic metrics derived from settlement receipts; these are the same samples that are held alongside the executor intent (`governor/decisions/epoch-*.json`) for audit.
  - `economics_epoch_tx_count`, `economics_epoch_tx_volume_block`, and `economics_epoch_treasury_inflow_block` capture the network activity, volume, and treasury inflow that feed the control loop.
  - `tb-cli governor status --rpc <endpoint>` prints the `telemetry gauges (ppm)` section plus the `last_economics_snapshot_hash` so you can prove the Prometheus series comes from the same sample the governor evaluated.
  - Shadow mode is the default: set `TB_GOVERNOR_SHADOW_ONLY=1` to keep intents and snapshot hashes flowing without mutating runtime params, then flip it to `0` once the telemetry streak looks healthy to allow apply.

- **Auditing workflow**
  1. After collecting the metrics you expect (via Grafana or Prometheus), copy the hash from `tb-cli governor status`. Compare it against the Blake3 hash stored inside `governor/decisions/epoch-*.json` to ensure the governor replayed the same deterministic sample that the telemetry gauges exposed.
  2. Use `tb-cli governor intents --gate economics` to see pending intents and their `snapshot_hash` lines. Each hash should match the corresponding decision file in `governor/decisions/`.
  3. If you need to inspect the actual sample JSON, cat the decision file; it contains the metrics (`market_metrics`, `epoch_treasury_inflow`, etc.) that triggered the gate.

- **Rollback play**
  1. Pause the governor by disabling it (`TB_GOVERNOR_ENABLED=0`) or shutting down the governor process; this prevents new intents from applying while you troubleshoot.
  2. If you were running in apply mode (`TB_GOVERNOR_SHADOW_ONLY=0`), flip back to shadow (`TB_GOVERNOR_SHADOW_ONLY=1`) so intents keep flowing for audit without mutating runtime parameters.
  3. To revert an applied gate, plan an exit intent (`GateAction::Exit`) by letting `tb-cli governor status` build up the required streak or by manually submitting the exit via the governor decision API. Confirm `economics_autopilot=false` in `tb-cli governor status`.
  4. Once the anomaly is addressed, re-enable the governor (`TB_GOVERNOR_ENABLED=1`) and replay the same metrics so the economics gate can re-enter from the known-good sample. Turn apply mode back on when you are ready to let the gate mutate runtime params again.

### Ad Market Quality + Cost Signals

- **Quality-adjusted pricing**
  - `ad_quality_multiplier_ppm{component}` reports freshness/privacy/readiness/overall multipliers (ppm).
  - `ad_quality_readiness_streak_windows` surfaces the readiness streak used for cohort quality.
  - `ad_quality_freshness_score_ppm` tracks the weighted freshness histogram score per presence bucket.
  - `ad_quality_privacy_score_ppm` tracks privacy budget headroom after denials/cooldowns.
- **Compute scarcity coupling**
  - `ad_compute_unit_price_usd_micros` is the compute-market spot price converted to USD micros.
  - `ad_cost_basis_usd_micros{component}` shows bandwidth/verifier/host/total floor components after scarcity coupling.
- **Tiered ad gates**
  - `ad_gate_ready_ppm{tier}` and `ad_gate_streak_windows{tier}` confirm contextual vs presence readiness streaks before apply.
  - Use `tb-cli governor status` to confirm the matching `ad_contextual`/`ad_presence` gate streaks and snapshot hashes.
- **Dashboards and wrappers**
  - Update `monitoring/ad_market_dashboard.json` and `metrics-aggregator` `/wrappers` snapshots whenever these series change; include refreshed panel screenshots (`npm ci --prefix monitoring && make monitor`) in reviews.

### Energy RPC Guardrails

- RPC calls enforce optional auth (`TB_ENERGY_RPC_TOKEN`) and a sliding-window rate limit (`TB_ENERGY_RPC_RPS`, default 50rps). Missing/incorrect tokens return `-33009`, while limits return `-33010`.
- Auth and rate checks happen before parsing business parameters, so rate spikes on unauthenticated traffic show up as `energy_signature_verification_failures_total` and the aggregator summary’s energy section.
- Aggregator `/wrappers` now includes `energy.rate_limit_rps` so dashboards can display the configured limit alongside dispute/settlement health.
- Keep these values in sync with downstream dashboards: the aggregator exposes `energy_*` counters in `/wrappers` and the Grafana energy board charts dispute counts, settlement backlog (`energy_pending_credits_total`), and signature failures.

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
contract-cli gov treasury balance | jq '{balance}'

# If insufficient, show what's waiting to execute
echo "=== Pending disbursements (sum of amounts) ==="
contract-cli gov treasury list --status queued --limit 200 | jq \
  '.disbursements | map(.amount) | add'
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
current=$(contract-cli gov treasury balance | jq .balance)
pending=$(contract-cli gov treasury list --status queued | jq '[.disbursements[].amount] | add')

echo "Current balance: $current BLOCK"
echo "Pending disbursements: $pending BLOCK"

if [ $current -lt $pending ]; then
  echo "INSUFFICIENT FUNDS"
  echo "Wait for accruals: watch -n 30 'contract-cli gov treasury balance | jq .balance'"
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

echo "=== Min-payment receipt rejections (0.001 BLOCK floor) ==="
prometheus_query 'receipt_min_payment_rejected_total'
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

## Explorer Treasury Schema Migration

Run this playbook whenever the explorer SQLite database still contains the legacy `amount`/`amount_it` columns in `treasury_disbursements`.

1. **Stop explorer** so the migration can take an exclusive lock on the DB file.
2. Run the helper (defaults to `explorer.db` in the current directory):
   ```bash
   cargo run -p explorer --bin explorer-migrate-treasury -- /var/lib/explorer/explorer.db
   ```
   The tool applies the three `ALTER TABLE` statements (`ADD COLUMN status_payload`, `RENAME COLUMN amount TO amount`, `DROP COLUMN amount_it`). Statements that have already landed are reported as `skipped`.
3. Restart explorer, then validate `/governance/treasury/disbursements` and the treasury dashboards before announcing completion.

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
  Initial balance: 10,000,000 BLOCK
  Accruals: 1,500,000 BLOCK
  Executed disbursements: 2,000,000 BLOCK
  Final balance: 9,500,000 BLOCK
```

**Failed audit** (example):
```
test settlement_audit ... FAILED

Assertion failed:
  Expected balance: 9,500,000 BLOCK
  Actual balance: 9,300,000 BLOCK
  Discrepancy: 200,000 BLOCK (2.1%)

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
| Min-Payment Rejections | Near-zero | Sudden increase in `receipt_min_payment_rejected_total` |
| Storage | All receipts | Storage > 1 week |
| Query Latency p99 | < 100ms | > 500ms for 5 min |

---

**Last Updated**: 2025-12-19  
**Next Review**: 2025-12-26  
**Maintainer**: Operations Team  
