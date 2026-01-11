# NEXT DEV: THREE BIG STRIDES - AUTHORITATIVE TECHNICAL DIRECTION

**Date**: 2025-12-19
**Session Context**: Post-treasury dependency parser consolidation + circuit breaker + telemetry implementation
**Target Audience**: Next developer (dev-to-dev, no BS)
**Status**: Circuit breaker implemented but NOT integrated into executor loop

---

## CODEBASE STATE ANALYSIS

### What EXISTS and is COMPLETE ✅

1. **Circuit Breaker Implementation** - `governance/src/circuit_breaker.rs` (440 lines)
   - Fully implemented with Closed/Open/HalfOpen states
   - Comprehensive telemetry using first-party `diagnostics` crate
   - 10 passing unit tests covering all state transitions
   - **EXPORTED** from governance crate at `governance/src/lib.rs:40`
   - **NOT YET INTEGRATED** into treasury executor loop

2. **Dependency Parser** - `governance/src/treasury.rs:480-529`
   - Canonical implementation with DOS prevention (MAX_DEPENDENCIES=100, 8KB memo limit)
   - Used by `node/src/treasury_executor.rs:43` via `parse_dependency_list()`
   - Deduplicates dependencies, handles JSON and key-value formats
   - 7 passing unit tests

3. **Security Hardening** - Multiple files
   - Treasury executor DOS prevention at `node/src/treasury_executor.rs:29` (MAX_MEMO_SIZE=1024)
   - Telemetry cardinality limits at `node/src/telemetry/treasury.rs:22-59`
   - Memo truncation in signer closure at `node/src/treasury_executor.rs:136-141`

4. **Stress Tests** - `tests/integration/` (3 files)
   - `treasury_stress_test.rs`: 10 scenarios, 100+ TPS baseline
   - `treasury_extreme_stress_test.rs`: 6 scenarios, 10k+ TPS target
   - **NOT YET VALIDATED** (tests exist but not confirmed to pass at scale)

5. **Monitoring Infrastructure** - `monitoring/` (3 files)
   - `prometheus_recording_rules.yml`: 9 rule groups, 50+ pre-computed metrics
   - `alert_rules.yml`: 15+ alerts with severity levels
   - `alertmanager.yml`: 6 receivers (PagerDuty, Slack, Email, Webhook, Opsgenie, VictorOps)
   - **CRITICAL GAP**: `treasury_circuit_breaker_state` metric referenced in `alert_rules.yml:17` but NOT IMPLEMENTED in telemetry module

6. **Operational Documentation** - `docs/` (2 files)
   - `DISASTER_RECOVERY.md`: 447 lines, backup/restore/failover procedures
   - `MULTI_NODE_TESTING.md`: 708 lines, 1 PC + 2 Mac M1 Air cluster setup

### What is MISSING or INCOMPLETE ❌

1. **Circuit Breaker Integration into Executor**
   - Circuit breaker exists but is NOT instantiated in executor
   - `governance/src/store.rs:175-322` (`run_executor_tick`) has NO circuit breaker logic
   - Need to wrap execution attempts with `allow_request()` / `record_success()` / `record_failure()`
   - Need to add circuit breaker to `TreasuryExecutorConfig` struct

2. **Circuit Breaker Telemetry Metrics**
   - `node/src/telemetry/treasury.rs` does NOT export `treasury_circuit_breaker_state` gauge
   - Alert rule at `monitoring/alert_rules.yml:17` will FAIL (references non-existent metric)
   - Need to add Prometheus gauge for circuit state (0=Closed, 1=Open, 2=HalfOpen)
   - Need to export public function to update gauge from executor

3. **Executor Loop Observability Gap**
   - `run_executor_tick()` at `governance/src/store.rs:290-311` has submission error handling
   - NO telemetry calls to increment error counters or record circuit breaker state changes
   - Submission errors at line 300-310 should trigger `record_failure()` on circuit breaker
   - Successful submissions at line 291-298 should trigger `record_success()` on circuit breaker

4. **Documentation Token Terminology Inconsistency**
   - 30+ markdown files in `docs/` still reference legacy “consumer/industrial token” wording
   - Should describe BLOCK as the canonical currency and treat consumer/industrial as lane labels, not separate tokens
   - Files identified: `docs/operations.md`, `docs/economics_and_governance.md`, etc.
   - **LOW PRIORITY** but affects documentation quality

5. **Stress Test Validation**
   - Tests exist but have NOT been run to completion
   - Need to verify 10k+ TPS capability under `treasury_extreme_stress_test.rs`
   - Tests are marked `#[ignore]` - must use `--ignored` flag
   - Background task `b9058b5` was still running when session ended

6. **Monitoring Stack Deployment**
   - Configuration files exist but are NOT deployed
   - No evidence of Prometheus running with recording rules
   - No evidence of AlertManager configured with receivers
   - No evidence of Grafana dashboards imported

---

## STRIDE 1: CIRCUIT BREAKER INTEGRATION & TELEMETRY (HIGHEST PRIORITY)

**Goal**: Wire circuit breaker into treasury executor loop with full observability
**Impact**: Production stability, cascading failure prevention, operational visibility
**Estimated Complexity**: MEDIUM (architectural changes, threading considerations)

### 1.1 Add Circuit Breaker to TreasuryExecutorConfig

**File**: `governance/src/store.rs`
**Location**: Lines 75-95 (struct `TreasuryExecutorConfig`)

**Current State**:
```rust
pub struct TreasuryExecutorConfig {
    pub identity: String,
    pub poll_interval: Duration,
    pub lease_ttl: Duration,
    pub epoch_source: Arc<dyn Fn() -> u64 + Send + Sync>,
    pub signer: Arc<dyn Fn(&TreasuryDisbursement) -> Result<SignedExecutionIntent, TreasuryExecutorError> + Send + Sync>,
    pub submitter: Arc<dyn Fn(&SignedExecutionIntent) -> Result<String, TreasuryExecutorError> + Send + Sync>,
    pub dependency_check: Option<Arc<dyn Fn(&GovStore, &TreasuryDisbursement) -> Result<bool, TreasuryExecutorError> + Send + Sync>>,
    pub nonce_floor: Arc<AtomicU64>,
}
```

**Required Change**:
```rust
use crate::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};

pub struct TreasuryExecutorConfig {
    // ... existing fields ...
    pub circuit_breaker: Arc<CircuitBreaker>,  // ADD THIS FIELD
}
```

**Rationale**:
- Circuit breaker must be `Arc<CircuitBreaker>` for thread-safe shared access
- Executor loop runs in separate thread (spawned at `store.rs:3251`)
- Multiple executor instances may run concurrently (lease-based coordination)

**Decision Point for Next Dev**:
- **Option A**: Add as required field (breaking change to all `spawn_executor()` call sites)
- **Option B**: Add as `Option<Arc<CircuitBreaker>>` (non-breaking, allows gradual rollout)
- **Recommendation**: Option A - fail fast, ensure circuit breaker is always present

### 1.2 Update spawn_treasury_executor Call Sites

**File**: `node/src/treasury_executor.rs`
**Location**: Lines 226-260 (function `spawn_executor`)

**Current State**:
```rust
pub fn spawn_executor(
    store: &GovStore,
    blockchain: Arc<Mutex<Blockchain>>,
    params: ExecutorParams,
) -> TreasuryExecutorHandle {
    let config = TreasuryExecutorConfig {
        identity,
        poll_interval,
        lease_ttl,
        epoch_source,
        signer,
        submitter,
        dependency_check: Some(dependency_check),
        nonce_floor,
    };
    store.spawn_treasury_executor(config)
}
```

**Required Change**:
```rust
use governance::{CircuitBreaker, CircuitBreakerConfig};

pub fn spawn_executor(
    store: &GovStore,
    blockchain: Arc<Mutex<Blockchain>>,
    params: ExecutorParams,
) -> TreasuryExecutorHandle {
    // Instantiate circuit breaker with production config
    let circuit_breaker_config = CircuitBreakerConfig {
        failure_threshold: 5,      // Open after 5 consecutive failures
        success_threshold: 2,       // Close after 2 consecutive successes in half-open
        timeout_secs: 60,          // Stay open for 60 seconds before attempting recovery
        window_secs: 300,          // 5 minute failure window
    };
    let circuit_breaker = Arc::new(CircuitBreaker::new(circuit_breaker_config));

    let config = TreasuryExecutorConfig {
        identity,
        poll_interval,
        lease_ttl,
        epoch_source,
        signer,
        submitter,
        dependency_check: Some(dependency_check),
        nonce_floor,
        circuit_breaker,  // ADD THIS
    };
    store.spawn_treasury_executor(config)
}
```

**Configuration Tuning Decision Points**:
- `failure_threshold: 5` - How many failures before opening?
  - **Lower** = more sensitive, faster failover, potential false positives
  - **Higher** = more tolerant, slower failover, risk of cascade
  - **Recommendation**: Start with 5, monitor alert frequency, adjust based on ops data

- `timeout_secs: 60` - How long to stay open?
  - **Shorter** = faster recovery attempts, risk of flapping
  - **Longer** = more stable, slower recovery from transient issues
  - **Recommendation**: 60s for initial deployment, consider exponential backoff in future

### 1.3 Integrate Circuit Breaker into Executor Loop

**File**: `governance/src/store.rs`
**Location**: Lines 175-322 (function `run_executor_tick`)

**Current Critical Section** (lines 290-311):
```rust
match (config.submitter)(&intent) {
    Ok(tx_hash) => {
        store.execute_disbursement(disbursement.id, &tx_hash, Vec::new())?;
        let _ = store.remove_execution_intent(disbursement.id);
        store.record_executor_nonce(&config.identity, intent.nonce)?;
        snapshot.record_nonce(intent.nonce);
        config.nonce_floor.store(intent.nonce, AtomicOrdering::SeqCst);
    }
    Err(err) => {
        if err.is_storage() {
            return Err(err);
        }
        if err.is_cancelled() {
            store.cancel_disbursement(disbursement.id, err.message())?;
            let _ = store.remove_execution_intent(disbursement.id);
        } else {
            last_error = Some(err.message().to_string());
        }
    }
}
```

**Required Architectural Changes**:

**A. Add Circuit Breaker Check at Loop Start** (insert after line 209):
```rust
let current_epoch = (config.epoch_source)();

// NEW: Check circuit breaker state
if !config.circuit_breaker.allow_request() {
    let state = config.circuit_breaker.state();
    snapshot.record_error(
        format!("circuit_breaker_{:?}", state),
        0,
        staged_lookup.len() as u64
    );
    store.store_executor_snapshot(snapshot)?;
    // Update telemetry (see section 1.4)
    update_circuit_breaker_telemetry(&config.circuit_breaker);
    return Ok(());
}
```

**B. Record Success on Successful Submission** (replace lines 291-298):
```rust
Ok(tx_hash) => {
    store.execute_disbursement(disbursement.id, &tx_hash, Vec::new())?;
    let _ = store.remove_execution_intent(disbursement.id);
    store.record_executor_nonce(&config.identity, intent.nonce)?;
    snapshot.record_nonce(intent.nonce);
    config.nonce_floor.store(intent.nonce, AtomicOrdering::SeqCst);

    // NEW: Record success with circuit breaker
    config.circuit_breaker.record_success();
    update_circuit_breaker_telemetry(&config.circuit_breaker);
}
```

**C. Record Failure on Submission Errors** (replace lines 300-310):
```rust
Err(err) => {
    if err.is_storage() {
        // Storage errors are fatal - don't count against circuit breaker
        return Err(err);
    }
    if err.is_cancelled() {
        // Cancelled errors are expected (insufficient balance, etc.) - don't count against circuit breaker
        store.cancel_disbursement(disbursement.id, err.message())?;
        let _ = store.remove_execution_intent(disbursement.id);
    } else {
        // Transient submission errors - record failure
        last_error = Some(err.message().to_string());

        // NEW: Record failure with circuit breaker
        config.circuit_breaker.record_failure();
        update_circuit_breaker_telemetry(&config.circuit_breaker);

        // NEW: Increment telemetry counter
        #[cfg(feature = "telemetry")]
        increment_treasury_execution_errors(&err);
    }
}
```

**CRITICAL DECISION POINT**: Which errors count against circuit breaker?

**Error Classification**:
1. **Storage Errors** (`err.is_storage()` = true)
   - **DO NOT** count against circuit breaker
   - Fatal errors - executor should shut down
   - Examples: database corruption, disk full, permission denied

2. **Cancelled Errors** (`err.is_cancelled()` = true)
   - **DO NOT** count against circuit breaker
   - Expected operational conditions
   - Examples: insufficient balance, nonce conflict, invalid destination
   - Rationale: These are not service health issues

3. **Submission Errors** (everything else)
   - **DO** count against circuit breaker
   - Network/RPC/blockchain admission failures
   - Examples: RPC timeout, connection refused, blockchain congestion
   - Rationale: These indicate downstream service health issues

**Behavioral Invariant**:
- Circuit breaker protects against **cascading failures** in downstream services
- Circuit breaker does NOT protect against **application logic errors** (cancelled) or **infrastructure failures** (storage)
- After opening, executor continues polling but skips submission attempts (fail fast)

### 1.4 Add Circuit Breaker Telemetry Metrics

**File**: `node/src/telemetry/treasury.rs`
**Location**: After line 100 (in telemetry gauge section)

**Add Gauge Declaration**:
```rust
/// Circuit breaker state (0=Closed, 1=Open, 2=HalfOpen)
#[cfg(feature = "telemetry")]
static TREASURY_CIRCUIT_BREAKER_STATE: Lazy<Gauge> = Lazy::new(|| {
    foundation_telemetry::register_gauge!(
        "treasury_circuit_breaker_state",
        "Current state of treasury circuit breaker (0=Closed, 1=Open, 2=HalfOpen)"
    )
    .unwrap_or_else(|_| Gauge::placeholder())
});

/// Circuit breaker failure count
#[cfg(feature = "telemetry")]
static TREASURY_CIRCUIT_BREAKER_FAILURES: Lazy<Gauge> = Lazy::new(|| {
    foundation_telemetry::register_gauge!(
        "treasury_circuit_breaker_failures",
        "Current failure count in circuit breaker"
    )
    .unwrap_or_else(|_| Gauge::placeholder())
});

/// Circuit breaker success count (in half-open state)
#[cfg(feature = "telemetry")]
static TREASURY_CIRCUIT_BREAKER_SUCCESSES: Lazy<Gauge> = Lazy::new(|| {
    foundation_telemetry::register_gauge!(
        "treasury_circuit_breaker_successes",
        "Current success count in circuit breaker half-open state"
    )
    .unwrap_or_else(|_| Gauge::placeholder())
});
```

**Add Public Update Function** (at end of file before mod tests):
```rust
/// Update circuit breaker state telemetry
///
/// Call this after any circuit breaker state change to ensure Prometheus metrics
/// reflect current state. Must be called from executor context.
#[cfg(feature = "telemetry")]
pub fn update_circuit_breaker_telemetry(circuit_breaker: &governance::CircuitBreaker) {
    use governance::CircuitState;

    let state = circuit_breaker.state();
    let state_value = match state {
        CircuitState::Closed => 0.0,
        CircuitState::Open => 1.0,
        CircuitState::HalfOpen => 2.0,
    };

    TREASURY_CIRCUIT_BREAKER_STATE.set(state_value);
    TREASURY_CIRCUIT_BREAKER_FAILURES.set(circuit_breaker.failure_count() as f64);
    TREASURY_CIRCUIT_BREAKER_SUCCESSES.set(circuit_breaker.success_count() as f64);
}

#[cfg(not(feature = "telemetry"))]
pub fn update_circuit_breaker_telemetry(_circuit_breaker: &governance::CircuitBreaker) {
    // No-op when telemetry disabled
}
```

**Add Error Counter Helper**:
```rust
#[cfg(feature = "telemetry")]
pub fn increment_treasury_execution_errors(err: &governance::TreasuryExecutorError) {
    let reason = sanitize_error_reason_label(err.message());
    TREASURY_EXECUTION_ERRORS_TOTAL
        .with_label_values(&[reason])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_treasury_execution_errors(_err: &governance::TreasuryExecutorError) {
    // No-op
}
```

**Import at Top of File**:
```rust
#[cfg(feature = "telemetry")]
use governance::{CircuitBreaker, CircuitState};
```

### 1.5 Verify AlertManager Integration

**File**: `monitoring/alert_rules.yml`
**Location**: Lines 15-26 (TreasuryCircuitBreakerOpen alert)

**Current Alert**:
```yaml
- alert: TreasuryCircuitBreakerOpen
  expr: treasury_circuit_breaker_state == 1
  for: 1m
  labels:
    severity: critical
  annotations:
    summary: "Treasury circuit breaker OPEN"
    description: "Circuit breaker opened after {{ $value }} failures"
    runbook_url: "https://docs.theblock.example.com/runbooks/treasury-circuit-breaker"
```

**Verification Steps**:
1. Start Prometheus with: `prometheus --config.file=monitoring/prometheus.yml`
2. Navigate to Prometheus UI: `http://localhost:9090`
3. Query: `treasury_circuit_breaker_state` - should return current state
4. Query: `treasury_circuit_breaker_failures` - should return failure count
5. Manually trigger circuit breaker open (inject failures in test)
6. Verify alert fires in Prometheus Alerts UI
7. Verify alert routes to AlertManager: `http://localhost:9093`
8. Verify alert routes to configured receivers (check Slack/PagerDuty)

**Alert Tuning Decision Points**:
- `for: 1m` - Alert fires after circuit open for 1 minute
  - **Shorter**: Faster incident response, potential alert fatigue
  - **Longer**: Reduced alert fatigue, delayed response
  - **Recommendation**: 1m for initial deployment, tune based on ops experience

### 1.6 Testing Strategy

**Unit Tests** (add to `governance/src/store.rs` tests section):
```rust
#[cfg(test)]
mod executor_circuit_breaker_tests {
    use super::*;

    #[test]
    fn circuit_breaker_opens_after_threshold_failures() {
        // Create test store with circuit breaker
        // Inject submission failures
        // Assert circuit opens after threshold
        // Assert subsequent ticks fail fast (no submission attempts)
    }

    #[test]
    fn circuit_breaker_transitions_to_half_open_after_timeout() {
        // Open circuit
        // Wait for timeout
        // Assert next tick attempts submission (half-open)
    }

    #[test]
    fn circuit_breaker_closes_after_successes_in_half_open() {
        // Force circuit to half-open
        // Inject successful submissions
        // Assert circuit closes after threshold successes
    }
}
```

**Integration Tests** (add to `tests/integration/treasury_lifecycle_test.rs`):
```rust
#[test]
fn treasury_executor_circuit_breaker_integration() {
    // Spawn real executor with circuit breaker
    // Simulate blockchain unavailability (RPC timeouts)
    // Assert circuit opens
    // Restore blockchain availability
    // Assert circuit recovers (half-open -> closed)
    // Assert successful disbursement execution after recovery
}
```

**Manual Testing Checklist**:
1. [ ] Start node with treasury executor enabled
2. [ ] Submit disbursements via RPC
3. [ ] Simulate blockchain failure (kill blockchain, firewall RPC port)
4. [ ] Observe circuit breaker open (check logs for `OPENED` message)
5. [ ] Observe telemetry metrics update (Prometheus query)
6. [ ] Observe alert fire (AlertManager UI)
7. [ ] Restore blockchain availability
8. [ ] Observe circuit breaker half-open (logs show `HALF-OPEN`)
9. [ ] Observe successful disbursement after recovery
10. [ ] Observe circuit breaker close (logs show `CLOSED`)

---

## STRIDE 2: STRESS TEST VALIDATION & PERFORMANCE PROFILING (MEDIUM PRIORITY)

**Goal**: Validate 10k+ TPS capability, identify bottlenecks, establish performance baseline
**Impact**: Scalability confidence, production capacity planning, optimization targets
**Estimated Complexity**: LOW-MEDIUM (mostly execution, some profiling/analysis)

### 2.1 Execute Stress Tests

**Test Files**:
1. `tests/integration/treasury_stress_test.rs` - 10 scenarios, 100+ TPS baseline
2. `tests/integration/treasury_extreme_stress_test.rs` - 6 scenarios, 10k+ TPS target

**Execution Commands**:
```bash
# Baseline stress tests (100+ TPS)
cargo test --release --test treasury_stress_test -- --ignored --nocapture --test-threads=1

# Extreme stress tests (10k+ TPS)
cargo test --release --test treasury_extreme_stress_test -- --ignored --nocapture --test-threads=1
```

**CRITICAL**: Use `--test-threads=1` to avoid concurrent test interference

**Expected Outcomes**:
- **Baseline**: All 10 scenarios complete with TPS > 100
- **Extreme**: All 6 scenarios complete with TPS > 10,000

**If Tests FAIL**:
1. **Identify Bottleneck**:
   - CPU saturation? (profile with `cargo flamegraph`)
   - Lock contention? (check for Mutex/RwLock hot paths)
   - Database I/O? (check sled flush behavior)
   - Memory allocation? (profile with `heaptrack` or `valgrind --tool=massif`)

2. **Optimization Targets** (in priority order):
   a. **Dependency Parser** (`governance/src/treasury.rs:480-529`)
      - Current: O(n) JSON parsing per disbursement
      - Optimization: Cache parsed dependencies in `TreasuryDisbursement` struct

   b. **Executor Batching** (`governance/src/store.rs:214-266`)
      - Current: MAX_BATCH_SIZE=100, MAX_SCAN_SIZE=500
      - Optimization: Increase batch size, implement parallel signing

   c. **Intent Staging** (`governance/src/store.rs:218-222`)
      - Current: Load all intents into HashMap every tick
      - Optimization: Maintain in-memory cache, only reload on miss

   d. **Database Flushes** (sled default: sync on write)
      - Current: Every disbursement write triggers fsync
      - Optimization: Batch writes, configure `flush_every_ms`

3. **Document Findings**:
   - Create `STRESS_TEST_RESULTS.md` with:
     - Test execution timestamp
     - Hardware specs (CPU, RAM, disk type)
     - Per-scenario TPS measurements
     - Bottleneck analysis (flamegraph screenshots)
     - Optimization recommendations with estimated impact

### 2.2 Stress Test Under Circuit Breaker Conditions

**New Test Scenario** (add to `treasury_extreme_stress_test.rs`):
```rust
#[test]
#[ignore]
fn test_circuit_breaker_under_extreme_load() {
    // Spawn executor with circuit breaker
    // Submit 100k disbursements
    // Inject intermittent blockchain failures (10% error rate)
    // Assert circuit breaker opens/closes correctly
    // Assert throughput degrades gracefully (no crash)
    // Assert recovery after failures stop
    // Measure: TPS during normal, open, half-open, recovery phases
}
```

**Success Criteria**:
- Circuit opens within 5 failures (as configured)
- TPS drops to ~0 when circuit open (fail fast)
- Circuit attempts recovery after timeout
- TPS recovers to >80% baseline after circuit closes
- **NO executor crashes or deadlocks**

### 2.3 Multi-Core Scalability Test

**Current Architecture**:
- Executor runs in single thread (spawned at `governance/src/store.rs:3251`)
- Signing is synchronous (blocks on `config.signer()` at line 285)
- Submission is synchronous (blocks on `config.submitter()` at line 290)

**Scalability Test**:
```rust
#[test]
#[ignore]
fn test_multi_core_scaling() {
    let cpus = num_cpus::get();

    for thread_count in [1, 2, 4, 8, cpus] {
        // Configure thread pool for signing/submission
        // Run 60-second sustained load test
        // Measure TPS
        // Assert: TPS scales linearly up to 4 cores
        // Assert: TPS plateaus beyond 4 cores (identify bottleneck)
    }
}
```

**If Scaling Poor**:
- **Likely Bottleneck**: Sled database lock contention
- **Solution**: Implement executor sharding (multiple executors with ID range partitions)
- **Alternative**: Switch to parallelized signing (batch sign 100 txs concurrently)

### 2.4 Dependency Graph Stress Test

**Test Scenario**:
```rust
#[test]
#[ignore]
fn test_deep_dependency_chains() {
    // Create 1000 disbursements with 10-level deep dependency chains
    // Example: D1000 depends on D999, D999 depends on D998, ..., D2 depends on D1
    // Submit all at once
    // Assert: Executor processes in correct topological order
    // Assert: No deadlocks or infinite loops
    // Measure: Processing time for deep chain vs flat (no deps)
}
```

**Current Implementation Concerns**:
- `dependencies_ready()` at `node/src/treasury_executor.rs:31-70` has O(n*m) complexity
  - n = number of dependencies per disbursement (max 100)
  - m = total disbursements in store (unbounded)
- For 10k disbursements with 10 deps each: 100k lookups per tick
- **Optimization**: Build dependency graph once, cache ready set

---

## STRIDE 3: OPERATIONAL READINESS & DOCUMENTATION (LOWER PRIORITY)

**Goal**: Production deployment readiness, operational playbooks, documentation consistency
**Impact**: Operational confidence, incident response speed, team onboarding
**Estimated Complexity**: LOW (mostly documentation, some scripting)

### 3.1 Deploy Monitoring Stack

**Components**:
1. **Prometheus** - Metrics collection and alerting
2. **AlertManager** - Alert routing and notification
3. **Grafana** - Dashboard visualization

**Deployment Steps**:

**A. Prometheus Setup**:
```bash
# Download Prometheus
wget https://github.com/prometheus/prometheus/releases/download/v2.48.0/prometheus-2.48.0.linux-amd64.tar.gz
tar xvfz prometheus-2.48.0.linux-amd64.tar.gz
cd prometheus-2.48.0.linux-amd64

# Copy config and rules
cp /Users/ianreitsma/projects/the-block/monitoring/prometheus_recording_rules.yml ./recording_rules.yml
cp /Users/ianreitsma/projects/the-block/monitoring/alert_rules.yml ./alert_rules.yml

# Create prometheus.yml (minimal config)
cat > prometheus.yml <<EOF
global:
  scrape_interval: 15s
  evaluation_interval: 15s

rule_files:
  - "recording_rules.yml"
  - "alert_rules.yml"

alerting:
  alertmanagers:
    - static_configs:
        - targets:
            - localhost:9093

scrape_configs:
  - job_name: 'theblock'
    static_configs:
      - targets: ['localhost:9090']  # Adjust to node metrics endpoint
EOF

# Start Prometheus
./prometheus --config.file=prometheus.yml --storage.tsdb.path=data/ --web.console.templates=consoles --web.console.libraries=console_libraries
```

**B. AlertManager Setup**:
```bash
# Download AlertManager
wget https://github.com/prometheus/alertmanager/releases/download/v0.26.0/alertmanager-0.26.0.linux-amd64.tar.gz
tar xvfz alertmanager-0.26.0.linux-amd64.tar.gz
cd alertmanager-0.26.0.linux-amd64

# Copy config
cp /Users/ianreitsma/projects/the-block/monitoring/alertmanager.yml ./alertmanager.yml

# CRITICAL: Update alertmanager.yml with REAL credentials
# - PagerDuty integration key
# - Slack webhook URL
# - Email SMTP settings
# - Opsgenie API key

# Start AlertManager
./alertmanager --config.file=alertmanager.yml --storage.path=data/
```

**C. Grafana Setup**:
```bash
# Install Grafana (Ubuntu/Debian)
sudo apt-get install -y software-properties-common
sudo add-apt-repository "deb https://packages.grafana.com/oss/deb stable main"
wget -q -O - https://packages.grafana.com/gpg.key | sudo apt-key add -
sudo apt-get update
sudo apt-get install grafana

# Start Grafana
sudo systemctl start grafana-server
sudo systemctl enable grafana-server

# Access Grafana UI: http://localhost:3000 (default: admin/admin)
# Add Prometheus data source: http://localhost:9090
# Import dashboards from monitoring/ directory
```

**Verification Checklist**:
1. [ ] Prometheus UI accessible (http://localhost:9090)
2. [ ] Prometheus targets "Up" (Status > Targets)
3. [ ] Recording rules evaluating (Status > Rules)
4. [ ] Alert rules loaded (Alerts page shows TreasuryCircuitBreakerOpen)
5. [ ] AlertManager UI accessible (http://localhost:9093)
6. [ ] AlertManager receivers configured (check silence/route test)
7. [ ] Grafana UI accessible (http://localhost:3000)
8. [ ] Grafana connected to Prometheus (data source test succeeds)
9. [ ] Treasury dashboard imported and rendering

**Troubleshooting**:
- **Prometheus scrape fails**: Check node metrics endpoint, verify port accessibility
- **Alert never fires**: Check alert expression in Prometheus > Alerts > Evaluate
- **AlertManager silent**: Check `alertmanager.yml` route config, verify receiver credentials
- **Grafana no data**: Check data source URL, verify Prometheus query syntax

### 3.2 Documentation Token Terminology Audit

**Files Identified** (30+ files in `docs/`):
- `docs/operations.md`
- `docs/economics_and_governance.md`
- `docs/economics_operator_runbook.md`
- `docs/ECONOMIC_SYSTEM_CHANGELOG.md`
- `docs/subsystem_atlas.md`
- `docs/world-os-spec/*.md` (multiple files)
- (Full list from grep result above)

**Find/Replace Strategy**:
```bash
# Find all occurrences (for review)
rg -i "consumer token|industrial token|dual token|BLOCK/IT" docs/ --glob "*.md" --color always

# CRITICAL: Manual review required - many false positives
# - Avoid standalone "IT" searches; they match unrelated words.
# - "BLOCK" matches "CONNECTION", "SELECT", etc.

# Recommended approach:
# 1. Search for full phrases first:
rg "Consumer Token" docs/ --glob "*.md"
rg "Industrial Token" docs/ --glob "*.md"

# 2. Review each occurrence in context
# 3. Replace ONLY when referring to token type
# 4. Prefer "BLOCK" with a parenthetical lane descriptor (e.g., BLOCK (consumer lane))
# 5. Keep lane labels (`consumer`, `industrial`) as routing hints in code snippets, not as standalone tokens

# Example replacements:
# "Consumer Tokens (BLOCK)" -> "BLOCK (consumer lane share)"
# "BLOCK balance" -> "BLOCK balance"
# "Industrial Token allocation" -> "BLOCK (industrial lane share)"
```

**Validation**:
```bash
# After replacements, verify no incorrect replacements
rg -i "consumer token|industrial token" docs/ --glob "*.md" | wc -l
# Should be 0 or very close to 0

# Check for consistent terminology
# Ensure no "TB" or "The Block Token" references remain
rg -i "The Block Token|TB Token" docs/ --glob "*.md" | wc -l
# Should be zero
```

**Documentation Update PR Checklist**:
1. [ ] All "Consumer Token" -> "BLOCK (consumer lane share)" (first mention)
2. [ ] All "Industrial Token" -> "BLOCK (industrial lane share)" (first mention)
3. [ ] All references to the ledger currency use BLOCK (not legacy token nicknames)
4. [ ] Reserve `IT` for the industrial lane label in code examples, not as a standalone token
5. [ ] Updated glossary (if exists) with BLOCK definition
6. [ ] Reviewed diffs manually (no false positive replacements)
7. [ ] Built documentation locally (check for broken links)
8. [ ] Ran spellcheck (no new typos introduced)

### 3.3 Multi-Node Testing Infrastructure

**Reference**: `docs/MULTI_NODE_TESTING.md` (708 lines)

**Architecture**:
- **Primary Node**: 1 PC (static IP: 192.168.1.10)
- **Replica Node 1**: Mac M1 Air (static IP: 192.168.1.11)
- **Replica Node 2**: Mac M1 Air (static IP: 192.168.1.12)

**Setup Script** (from `MULTI_NODE_TESTING.md` section 4):
```bash
#!/bin/bash
# deploy_multi_node.sh - Automated 3-node cluster setup

set -e

PRIMARY_IP="192.168.1.10"
REPLICA1_IP="192.168.1.11"
REPLICA2_IP="192.168.1.12"

# Generate node-specific configs
generate_config() {
    local node_type=$1  # primary, replica1, replica2
    local node_ip=$2

    cat > "config_${node_type}.toml" <<EOF
[network]
listen_addr = "${node_ip}:9000"
advertise_addr = "${node_ip}:9000"

[peers]
bootstrap = [
    "${PRIMARY_IP}:9000",
    "${REPLICA1_IP}:9000",
    "${REPLICA2_IP}:9000"
]

[storage]
data_dir = "/var/lib/theblock/${node_type}"

[treasury]
executor_enabled = $([ "$node_type" = "primary" ] && echo "true" || echo "false")
executor_identity = "executor-${node_type}"
EOF
}

# Generate configs
generate_config "primary" "$PRIMARY_IP"
generate_config "replica1" "$REPLICA1_IP"
generate_config "replica2" "$REPLICA2_IP"

# Deploy to nodes (assumes SSH access)
scp config_primary.toml user@${PRIMARY_IP}:/etc/theblock/config.toml
scp config_replica1.toml user@${REPLICA1_IP}:/etc/theblock/config.toml
scp config_replica2.toml user@${REPLICA2_IP}:/etc/theblock/config.toml

# Start nodes
ssh user@${PRIMARY_IP} "sudo systemctl restart theblock"
ssh user@${REPLICA1_IP} "sudo systemctl restart theblock"
ssh user@${REPLICA2_IP} "sudo systemctl restart theblock"

# Verify cluster health
sleep 10
ssh user@${PRIMARY_IP} "curl -s http://localhost:9090/health | jq"
ssh user@${REPLICA1_IP} "curl -s http://localhost:9090/health | jq"
ssh user@${REPLICA2_IP} "curl -s http://localhost:9090/health | jq"

echo "Multi-node cluster deployed successfully!"
```

**Test Scenarios** (from `MULTI_NODE_TESTING.md` section 5):

**Scenario 1: Baseline Consensus**
```bash
# Submit 1000 disbursements via primary
for i in {1..1000}; do
    curl -X POST http://${PRIMARY_IP}:9090/treasury/disbursement \
        -H "Content-Type: application/json" \
        -d "{\"destination\": \"addr_$i\", \"amount\": 100}"
done

# Verify all nodes have same treasury state
for node_ip in $PRIMARY_IP $REPLICA1_IP $REPLICA2_IP; do
    curl -s http://${node_ip}:9090/treasury/balance | jq '.balance'
done

# Assert: All nodes return same balance
```

**Scenario 2: Primary Failover**
```bash
# Kill primary node
ssh user@${PRIMARY_IP} "sudo systemctl stop theblock"

# Submit disbursements to replica
curl -X POST http://${REPLICA1_IP}:9090/treasury/disbursement \
    -H "Content-Type: application/json" \
    -d '{"destination": "addr_failover", "amount": 500}'

# Verify executor lease transfers to replica
curl -s http://${REPLICA1_IP}:9090/treasury/executor/status | jq '.lease_holder'

# Restore primary
ssh user@${PRIMARY_IP} "sudo systemctl start theblock"

# Verify primary syncs state
sleep 30
curl -s http://${PRIMARY_IP}:9090/treasury/balance | jq '.balance'
```

**Scenario 3: Network Partition**
```bash
# Partition: Primary vs. (Replica1 + Replica2)
ssh user@${PRIMARY_IP} "sudo iptables -A OUTPUT -d ${REPLICA1_IP} -j DROP"
ssh user@${PRIMARY_IP} "sudo iptables -A OUTPUT -d ${REPLICA2_IP} -j DROP"

# Submit disbursements to both partitions
curl -X POST http://${PRIMARY_IP}:9090/treasury/disbursement \
    -d '{"destination": "addr_p1", "amount": 100}'

curl -X POST http://${REPLICA1_IP}:9090/treasury/disbursement \
    -d '{"destination": "addr_p2", "amount": 200}'

# Heal partition
ssh user@${PRIMARY_IP} "sudo iptables -D OUTPUT -d ${REPLICA1_IP} -j DROP"
ssh user@${PRIMARY_IP} "sudo iptables -D OUTPUT -d ${REPLICA2_IP} -j DROP"

# Verify reconciliation (requires consensus algorithm details)
# Expected: Majority partition (Replica1+Replica2) wins
# Expected: Primary rolls back conflicting disbursements
```

**Automated Dashboard Deployment** (from `MULTI_NODE_TESTING.md` section 6):
```bash
#!/bin/bash
# deploy_dashboards.sh - Deploy Grafana dashboards to all nodes

for node_ip in $PRIMARY_IP $REPLICA1_IP $REPLICA2_IP; do
    # Install Grafana on node
    ssh user@${node_ip} "sudo apt-get update && sudo apt-get install -y grafana"

    # Configure Prometheus data source
    ssh user@${node_ip} "cat > /tmp/datasource.yml <<EOF
apiVersion: 1
datasources:
  - name: Prometheus
    type: prometheus
    url: http://localhost:9090
    access: proxy
    isDefault: true
EOF"
    ssh user@${node_ip} "sudo mv /tmp/datasource.yml /etc/grafana/provisioning/datasources/"

    # Copy dashboards
    scp monitoring/grafana_treasury_dashboard.json user@${node_ip}:/tmp/
    scp monitoring/grafana_energy_dashboard.json user@${node_ip}:/tmp/
    ssh user@${node_ip} "sudo mv /tmp/*.json /var/lib/grafana/dashboards/"

    # Restart Grafana
    ssh user@${node_ip} "sudo systemctl restart grafana-server"

    echo "Dashboards deployed to ${node_ip}"
done
```

### 3.4 Disaster Recovery Drill

**Reference**: `docs/DISASTER_RECOVERY.md` (447 lines)

**Drill Schedule**: Quarterly (Q1, Q2, Q3, Q4)

**Drill Procedure** (from `DISASTER_RECOVERY.md` section 5):

**A. Total Data Loss Scenario**:
```bash
# 1. Stop all nodes
for node_ip in $PRIMARY_IP $REPLICA1_IP $REPLICA2_IP; do
    ssh user@${node_ip} "sudo systemctl stop theblock"
done

# 2. Delete all data directories
for node_ip in $PRIMARY_IP $REPLICA1_IP $REPLICA2_IP; do
    ssh user@${node_ip} "sudo rm -rf /var/lib/theblock/*"
done

# 3. Restore from backup (assumes S3 backup available)
BACKUP_DATE=$(date -d "yesterday" +%Y-%m-%d)
aws s3 cp s3://theblock-backups/${BACKUP_DATE}/governance_db.tar.gz /tmp/
tar xzf /tmp/governance_db.tar.gz -C /var/lib/theblock/

# 4. Restart nodes
for node_ip in $PRIMARY_IP $REPLICA1_IP $REPLICA2_IP; do
    ssh user@${node_ip} "sudo systemctl start theblock"
done

# 5. Verify restoration
curl -s http://${PRIMARY_IP}:9090/treasury/balance | jq
# Assert: Balance matches pre-drill snapshot

# 6. Document RTO (Recovery Time Objective)
echo "Restoration completed in $(date -u -d @$(($(date +%s) - $START_TIME)) +%M:%S)"
```

**B. Executor Failure Scenario**:
```bash
# 1. Kill primary executor
ssh user@${PRIMARY_IP} "sudo kill -9 $(pgrep -f treasury_executor)"

# 2. Verify lease expires (TTL = 60 seconds)
sleep 70

# 3. Verify replica takes over
curl -s http://${REPLICA1_IP}:9090/treasury/executor/status | jq '.lease_holder'
# Assert: Replica1 or Replica2 now holds lease

# 4. Verify disbursements continue processing
BALANCE_BEFORE=$(curl -s http://${REPLICA1_IP}:9090/treasury/balance | jq '.balance')
sleep 30
BALANCE_AFTER=$(curl -s http://${REPLICA1_IP}:9090/treasury/balance | jq '.balance')
# Assert: BALANCE_AFTER != BALANCE_BEFORE (disbursements processed)

# 5. Restart primary executor
ssh user@${PRIMARY_IP} "sudo systemctl restart theblock"

# 6. Document RTO
echo "Failover completed in <70 seconds (lease TTL)"
```

**Drill Report Template**:
```markdown
# Disaster Recovery Drill Report

**Date**: YYYY-MM-DD
**Scenario**: [Total Data Loss / Executor Failure / Network Partition]
**Participants**: [List of engineers]

## Objectives
- [ ] Restore from backup within RTO (1 hour for critical systems)
- [ ] Verify data integrity (RPO: 15 minutes for treasury)
- [ ] Validate failover procedures
- [ ] Test alert/escalation procedures

## Timeline
| Time | Event |
|------|-------|
| T+0m | Drill initiated |
| T+5m | All nodes stopped |
| T+15m | Backup restoration started |
| T+45m | Nodes restarted |
| T+60m | Verification complete |

## Results
- **RTO Achieved**: [YES/NO] (Actual: XX minutes vs Target: 60 minutes)
- **RPO Achieved**: [YES/NO] (Data loss: XX minutes vs Target: 15 minutes)
- **Data Integrity**: [PASS/FAIL] (All balances match pre-drill snapshot)

## Issues Encountered
1. [Issue description]
2. [Issue description]

## Action Items
1. [Action item] - Owner: [Name] - Due: [Date]
2. [Action item] - Owner: [Name] - Due: [Date]

## Recommendations
- [Recommendation for process improvement]
- [Recommendation for infrastructure change]
```

---

## DECISION POINTS SUMMARY

**For Next Dev** - These require YOUR judgment based on production requirements:

### Circuit Breaker Configuration
1. **failure_threshold**: 5 (recommended) vs 3 (sensitive) vs 10 (tolerant)
2. **timeout_secs**: 60 (recommended) vs 30 (aggressive) vs 120 (conservative)
3. **Error Classification**: Which errors count against circuit? (storage=NO, cancelled=NO, submission=YES is recommended)

### Performance Tuning
1. **Executor Batch Size**: 100 (current) vs 500 (aggressive) vs 1000 (risky)
2. **Database Flush Policy**: sync_on_write (current, safe) vs flush_every_ms (faster, slight risk)
3. **Parallelization Strategy**: Single-threaded (current) vs executor sharding (complex) vs parallel signing (moderate)

### Operational Configuration
1. **Alert Thresholds**: 1m delay (recommended) vs 30s (sensitive) vs 5m (tolerant)
2. **Monitoring Retention**: 15 days (default) vs 90 days (compliance) vs 1 year (audit)
3. **Backup Frequency**: Daily (current) vs hourly (stringent) vs weekly (relaxed)

---

## FILES REQUIRING MODIFICATION

**Priority 1 (Circuit Breaker Integration)**:
1. `governance/src/store.rs` - Lines 75-95, 175-322 (struct + executor loop)
2. `node/src/treasury_executor.rs` - Lines 226-260 (spawn_executor)
3. `node/src/telemetry/treasury.rs` - Add circuit breaker metrics (after line 100)

**Priority 2 (Testing)**:
4. `tests/integration/treasury_extreme_stress_test.rs` - Run existing tests
5. `governance/src/store.rs` - Add circuit breaker unit tests (after line 3287)

**Priority 3 (Operational)**:
6. `monitoring/*.yml` - Deploy to Prometheus/AlertManager
7. `docs/*.md` - Token terminology updates (30+ files)

---

## COMPLETION CRITERIA

**Stride 1 Complete When**:
- [ ] Circuit breaker integrated into executor loop (compiles, no warnings)
- [ ] Telemetry metrics exported (`treasury_circuit_breaker_state` queryable in Prometheus)
- [ ] Unit tests pass (circuit breaker integration tests)
- [ ] Integration test passes (manual executor failover test)
- [ ] Alert fires correctly (manual trigger + verify AlertManager routing)

**Stride 2 Complete When**:
- [ ] All stress tests pass (`treasury_stress_test.rs` + `treasury_extreme_stress_test.rs`)
- [ ] TPS measurements documented (`STRESS_TEST_RESULTS.md`)
- [ ] Bottlenecks identified (flamegraph analysis complete)
- [ ] Circuit breaker stress test passes (graceful degradation verified)

**Stride 3 Complete When**:
- [ ] Monitoring stack deployed (Prometheus + AlertManager + Grafana running)
- [ ] Multi-node cluster deployed (3 nodes communicating)
- [ ] DR drill executed (report generated, RTO/RPO verified)
- [ ] Documentation audit complete (no "BLOCK" or "IT" token references remain)

---

## RISK ASSESSMENT

**High Risk Areas**:
1. **Thread Safety**: Circuit breaker shared across executor threads via `Arc` - verify no data races
2. **Database Contention**: Sled lock contention under high load - monitor with profiler
3. **Failover Timing**: Lease TTL (60s) vs circuit timeout (60s) - potential overlap, test edge cases
4. **Alert Fatigue**: Circuit may flap under intermittent failures - tune thresholds carefully

**Mitigation Strategies**:
1. **Thread Safety**: Use atomics + Mutex (already done), add ThreadSanitizer tests
2. **Database Contention**: Implement write batching, consider async sled operations
3. **Failover Timing**: Make circuit timeout configurable, decouple from lease TTL
4. **Alert Fatigue**: Implement alert grouping, use `for: 5m` instead of `for: 1m` initially

---

## REFERENCE ARCHITECTURE

**Current Executor Flow** (`governance/src/store.rs:175-322`):
```
1. Acquire lease (line 180-207)
2. Load disbursements (line 217)
3. Filter executable (line 225-256)
4. Batch processing (line 262-266)
5. For each disbursement:
   a. Check dependencies (line 274-278)
   b. Sign transaction (line 285-287)
   c. Submit transaction (line 290-311)
      - Success: Record nonce, remove intent (291-298)
      - Failure: Cancel or record error (300-310)
6. Update snapshot (line 314-320)
7. Sleep poll_interval (line 3276 in spawn function)
```

**Target Executor Flow with Circuit Breaker**:
```
1. Acquire lease
2. CHECK CIRCUIT BREAKER ← NEW
   - If OPEN: Skip to step 6 (fail fast)
   - If HALF-OPEN or CLOSED: Continue
3. Load disbursements
4. Filter executable
5. Batch processing
6. For each disbursement:
   a. Check dependencies
   b. Sign transaction
   c. Submit transaction
      - Success: Record nonce, RECORD SUCCESS ← NEW
      - Transient failure: RECORD FAILURE ← NEW
      - Cancelled: No circuit breaker update
      - Storage error: No circuit breaker update (fatal)
7. UPDATE TELEMETRY ← NEW
8. Update snapshot
9. Sleep poll_interval
```

---

**END OF TECHNICAL DIRECTION**

Next dev: You have complete information. Make decisions on configuration tuning, execute the three strides, document results. No ambiguity remains. Go build.
