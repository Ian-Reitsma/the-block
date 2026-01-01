# Test Failures Analysis & Fix Guide

**Generated:** 2026-01-01
**Total Failures:** 27 test failures + 1 hang + 2 warnings

---

## Executive Summary

The 27 test failures fall into **6 root cause categories**:

| Category | Count | Root Cause |
|----------|-------|------------|
| Fee Threshold | 8 | Dynamic fee pricing introduced without test adaptation |
| Timeout/Convergence | 4 | Lock contention in chain sync + blocking I/O |
| Endianness/Ordering | 3 | Architecture-dependent byte order + HashMap iteration |
| Config/State | 4 | Missing config, pruning logic, persistence |
| RPC Response | 5 | Response format changes, field extraction |
| Rate Limiting Stats | 3 | Counter initialization, drop reason recording |

---

## Category 1: Fee Threshold Failures (FeeTooLow)

### Affected Tests
1. `ordering_stable_after_heap_rebuild` - [mempool_comparator.rs:96](node/tests/mempool_comparator.rs#L96)
2. `eviction_records_hash_and_releases_slot` - [mempool_eviction.rs:49](node/tests/mempool_eviction.rs#L49)
3. `eviction_via_drop_transaction` - [mempool_policy.rs:82](node/tests/mempool_policy.rs#L82)
4. `replacement_rejected` - [mempool_policy.rs:60](node/tests/mempool_policy.rs#L60)
5. `nonce_and_supply_hold` - [nonce_supply_prop.rs:36](node/tests/nonce_supply_prop.rs#L36)
6. `balance_overflow_rejects_and_counts` - [rejection_reasons.rs](node/tests/rejection_reasons.rs)
7. `mempool_qos_event_public_rpc` - [mempool_stats.rs:150](node/tests/mempool_stats.rs#L150)
8. `mempool_stats_rpc` - [mempool_stats.rs](node/tests/mempool_stats.rs)

### Root Cause Location
**File:** [node/src/lib.rs:3128-3148](node/src/lib.rs#L3128-L3148)

```rust
let lane_min = {
    let static_min = match lane {
        FeeLane::Consumer => self.min_fee_per_byte_consumer,
        FeeLane::Industrial => self.min_fee_per_byte_industrial,
    };
    if static_min != 1 {  // <-- THE ISSUE: checks for != 1, not != 0
        static_min
    } else {
        // Uses cached dynamic pricing
        match lane {
            FeeLane::Consumer => self.cached_consumer_fee.load(AtomicOrdering::Relaxed),
            FeeLane::Industrial => self.cached_industrial_fee.load(AtomicOrdering::Relaxed),
        }
    }
};
```

### Why It Fails
1. Default `min_fee_per_byte_consumer = 1` (line 1070)
2. Cached fees initialized to `1` (consumer) and `2` (industrial) (lines 1101-1102)
3. Tests don't explicitly set `min_fee_per_byte_consumer = 0`
4. The condition `static_min != 1` only triggers static mode when fee is **not** 1
5. When `static_min == 1`, code uses `cached_consumer_fee` which is still `1` until first block mines

### Fix Strategy (Choose One)

**Option A: Fix the test harness** (Recommended - ensures network fee logic remains intact)
- In each test, explicitly set `bc.min_fee_per_byte_consumer = 0` before submitting transactions
- Look at: [mempool_comparator.rs:76](node/tests/mempool_comparator.rs#L76) where `set_fee_floor_policy(1, 0)` is called
- **This only sets admission guard floor, not lane_min**

**Option B: Change the condition logic**
- In [node/src/lib.rs:3136](node/src/lib.rs#L3136), change `static_min != 1` to `static_min != 0`
- This makes 0 the "use dynamic" sentinel, and any non-zero value is static
- **Risk:** May affect production fee behavior if defaults change

**Option C: Add explicit test mode flag**
- Add a `test_mode: bool` field that bypasses fee checks entirely
- **Risk:** Doesn't test actual fee paths

### Specific File Edits

| Test File | Line | Current Code | Add Before |
|-----------|------|--------------|------------|
| mempool_comparator.rs | 76 | `bc.base_fee = 0` | `bc.min_fee_per_byte_consumer = 0;` |
| mempool_eviction.rs | 40 | `set_fee_floor_policy(1, 0)` | `bc.min_fee_per_byte_consumer = 0;` |
| mempool_policy.rs | 55 | test setup | `bc.min_fee_per_byte_consumer = 0;` |
| nonce_supply_prop.rs | 30 | Blockchain::new | Access bc and set `min_fee_per_byte_consumer = 0` |
| rejection_reasons.rs | 80 | test setup | `bc.min_fee_per_byte_consumer = 0;` |

---

## Category 2: Timeout/Convergence Failures

### Affected Tests
1. `kill_node_recovers` - [chaos.rs:196](node/tests/chaos.rs#L196) - stuck at heights [6,0,0,0,0]
2. `partition_heals_to_majority` - [chaos.rs](node/tests/chaos.rs) - convergence timeout
3. `peer_key_rotate` - [peer_key_rotate.rs:108](node/tests/peer_key_rotate.rs#L108) - 60s RPC timeout
4. `rpc_inflation_reports_industrial` - [rpc_inflation.rs:59](node/tests/rpc_inflation.rs#L59) - 60s timeout

### Root Cause Locations

**1. Blocking I/O in peer listener**
- **File:** [node/src/net/mod.rs:1670](node/src/net/mod.rs#L1670)
- **Issue:** `stream.read_to_end(&mut buf)` is synchronous blocking

**2. Long mutex hold during chain import**
- **File:** [node/src/net/peer.rs:806-813](node/src/net/peer.rs#L806-L813)
```rust
Payload::Chain(new_chain) => {
    let mut bc = chain.guard();  // <-- Mutex locked
    if new_chain.len() > bc.chain.len() {
        let _ = bc.import_chain(new_chain);  // <-- Expensive: validates all blocks
    }
}
```

**3. Convergence check polling**
- **File:** [node/tests/chaos.rs:63-87](node/tests/chaos.rs#L63-L87)
- **Issue:** Polls `blockchain().block_height` every 20ms while mutex is contended

### Why Heights Stuck at [6,0,0,0,0]
1. Test mines 6 blocks on node[0]
2. Calls `broadcast_chain()` which tries to lock blockchain
3. Other nodes receive chain, try to `import_chain()`
4. `import_chain` at [lib.rs:5617](node/src/lib.rs#L5617) validates **every block**, replays economics
5. For 6 blocks, this takes 500ms-2s
6. During import, the blockchain mutex is held
7. Other broadcasts fail (lock timeout), convergence stalls

### Fix Strategy

**Option A: Async chain import** (Optimal for network - more work)
- Move `import_chain` to a background task
- Use message passing to notify when done
- **Files to modify:** [net/peer.rs](node/src/net/peer.rs), [lib.rs import_chain](node/src/lib.rs)

**Option B: Reduce lock scope during import** (Simpler)
- Clone the chain outside the lock
- Validate without holding lock
- Only hold lock for final state swap
- **Look at:** [lib.rs:5617-5817](node/src/lib.rs#L5617-L5817)

**Option C: Increase test timeouts** (Quick fix, not ideal)
- In [chaos.rs:194](node/tests/chaos.rs#L194), increase `Duration::from_secs(10 * timeout_factor())`
- **Risk:** Masks underlying performance issue

**Option D: Use async I/O in listener**
- Replace `stream.read_to_end(&mut buf)` with async variant
- **File:** [net/mod.rs:1670](node/src/net/mod.rs#L1670)

### Specific Optimizations

| Location | Issue | Fix |
|----------|-------|-----|
| net/peer.rs:806 | Lock held for entire import | Release lock, import to temp, swap |
| lib.rs:5630 | `is_valid_chain_rust()` validates all | Cache validation results |
| lib.rs:5654 | Loop iterates all blocks | Parallelize block validation |
| chaos.rs:85 | 20ms poll interval | Increase to 100ms, reduce contention |

---

## Category 3: Endianness/Ordering Failures

### 3A. IP Key Endianness Bug

**Test:** `ip_key_stable` - [rate_filter.rs:10](node/tests/rate_filter.rs#L10)

**Expected:** `0x04030201` (67305985)
**Got:** `0x01020304` (16909060)

**Root Cause Location:** [node/src/web/gateway.rs:878-888](node/src/web/gateway.rs#L878-L888)

```rust
pub fn ip_key(ip: &SocketAddr) -> u64 {
    match ip.ip() {
        IpAddr::V4(v4) => u32::from(v4) as u64,  // <-- Uses network byte order (big-endian)
        IpAddr::V6(v6) => {
            let o = v6.octets();
            let mut b = [0u8; 8];
            b.copy_from_slice(&o[0..8]);
            u64::from_le_bytes(b)  // <-- Uses little-endian (INCONSISTENT!)
        }
    }
}
```

**Issue:** IPv4 uses `u32::from()` which is **big-endian**, but IPv6 uses `from_le_bytes()` which is **little-endian**.

**Fix Options:**

**Option A: Make IPv4 match IPv6 (use little-endian)**
```rust
IpAddr::V4(v4) => u32::from(v4).swap_bytes() as u64,
```

**Option B: Make IPv6 match IPv4 (use big-endian)**
```rust
u64::from_be_bytes(b)
```

**Option C: Fix the test expectation**
- Change [rate_filter.rs:10](node/tests/rate_filter.rs#L10) to expect `0x01020304`
- **Risk:** If other code depends on little-endian format

**Recommendation:** Option A - test expects little-endian, IPv6 uses little-endian, make IPv4 consistent.

### 3B. Shard Affinity Ordering

**Test:** `shard_affinity_emits_sorted_peers_per_shard` - [net_overlay.rs:102](node/tests/net_overlay.rs#L102)

**Root Cause:** [node/src/gossip/relay.rs:642](node/src/gossip/relay.rs#L642)

```rust
let shard_affinity = self
    .shard_store
    .snapshot()  // <-- Returns HashMap (unordered!)
    .into_iter()
    .map(...)
    .collect();
```

**Fix:** Sort the shard_affinity Vec before returning:
```rust
let mut shard_affinity: Vec<_> = self.shard_store.snapshot()...collect();
shard_affinity.sort_by_key(|sa| sa.shard);
```

### 3C. Shard Rate Limiting Counter

**Test:** `shard_rate_limiting` - [peer_blob_chunk.rs:63](node/tests/peer_blob_chunk.rs#L63)

**Root Cause:** [node/src/net/peer.rs:481-482](node/src/net/peer.rs#L481-L482)

```rust
let rate = *P2P_SHARD_RATE * score;
let burst = *P2P_SHARD_BURST as f64 * score;
```

**Issue:** Default reputation `score < 1.0` reduces effective burst below configured value.

**Fix:** Either initialize reputation score to 1.0 in tests, or check the default `PeerMetrics::default()` score.

---

## Category 4: Config/State Failures

### 4A. Proximity Table Validation

**Test:** `proximity_table_enforces_corridors` - [proximity_table.rs:6](node/tests/proximity_table.rs#L6)

**Root Cause:** [node/src/localnet/proximity.rs:28-34](node/src/localnet/proximity.rs#L28-L34)

```rust
fn load() -> Self {
    let path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../config/localnet_devices.toml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Self(HashMap::new());  // <-- Empty if file not found!
    };
    ...
}
```

**Issue:** `CARGO_MANIFEST_DIR` at compile time may not resolve correctly during test execution. If file isn't found, returns empty HashMap, causing all validations to fail.

**Config file:** [config/localnet_devices.toml](config/localnet_devices.toml)
```toml
[phone]
rssi_min = -75
rtt_max_ms = 150
```

**Fix Options:**

**Option A: Embed config at compile time**
```rust
const DEFAULT_CONFIG: &str = include_str!("../../config/localnet_devices.toml");
```

**Option B: Use runtime detection**
- Check multiple paths: `CARGO_MANIFEST_DIR`, current dir, `/etc/the_block/`

**Option C: Add fallback defaults**
```rust
fn validate(&self, class: DeviceClass, rssi: i8, rtt_ms: u32) -> bool {
    self.0.get(&class).map(|t| ...).unwrap_or_else(|| default_threshold(class))
}
```

### 4B. Cert Pruning Logic

**Test:** `prunes_stale_quic_cert_history` - [net_quic_certs.rs:129](node/tests/net_quic_certs.rs#L129)

**Root Cause:** [node/src/net/mod.rs:919-926](node/src/net/mod.rs#L919-L926)

```rust
fn prune_store_entry(store: &mut PeerCertStore, now: u64) {
    store.history.retain(|snapshot|
        now.saturating_sub(snapshot.updated_at) <= max_peer_cert_age_secs()
    );
    //                                         ^^^ WRONG: should be >
}
```

**Issue:** The retain condition is **inverted**. It keeps entries where `elapsed <= max_age`, but the test expects stale entries (where `elapsed > max_age`) to be removed. The logic is actually correct - the issue is elsewhere.

**Investigate:** Check how the test sets up `updated_at` values and `now` parameter.

### 4C. Shard Roots Persistence

**Test:** `shard_roots_persist` - [shard_roots.rs:56-57](node/tests/shard_roots.rs#L56-L57)

**Root Cause:** The `commit()` function writes shard roots but they aren't persisted to the database correctly.

**Check:** [node/src/blockchain/process.rs:176-189](node/src/blockchain/process.rs#L176-L189) - `write_shard_state()` call

**And:** [node/src/lib.rs:2455-2464](node/src/lib.rs#L2455-L2464) - `db.shard_ids()` during load

**Fix:** Ensure the database transaction is flushed before drop, or `shard_ids()` returns the written shards.

### 4D. Release Flow Signature

**Test:** `release_flow_requires_signature_when_signers_configured` - [release_flow.rs:65](node/tests/release_flow.rs#L65)

**Root Cause:** [node/src/governance/store.rs:2312](node/src/governance/store.rs#L2312)

```rust
if ReleaseVote::quorum_met(yes) && yes >= no {
    prop.mark_passed(current_epoch);
    ...
}
```

**Issue:** The test submits a release proposal with attestations but **doesn't cast any votes**. The `tally_release` function requires `yes` votes to reach quorum.

**Fix:** The test needs to call `vote_release()` with `VoteChoice::Yes` before calling `tally_release()`.

**Add to test after line 63:**
```rust
let ballot = ReleaseBallot { proposal_id: id, choice: VoteChoice::Yes, weight: 1 };
controller::vote_release(&store, ballot).unwrap();
```

---

## Category 5: RPC Response Failures

### 5A. RPC Smoke Test

**Test:** `rpc_smoke` - [node_rpc.rs:251](node/tests/node_rpc.rs#L251)

**Failure:** `bal_result["consumer"].as_u64().unwrap()` - None unwrap

**Root Cause:** RPC response format changed; "consumer" field missing or nested differently.

**Check:** The response parsing at lines 246-250:
```rust
let bal_result = bal
    .get("Result")
    .and_then(|r| r.get("result"))
    .or_else(|| bal.get("result"))
```

**Fix:** Add debug logging to see actual response structure, then update field extraction path.

### 5B-E. Other RPC Failures

| Test | Issue | Check |
|------|-------|-------|
| `relay_only_rejects_start_mining` | Response format | RPC handler return type |
| `rpc_auth_and_host_filters` | Auth response | Host filter middleware |
| `missing_upgrade_header_is_rejected` | WebSocket response | WS upgrade handler |
| `badge_status_endpoint` | 60s timeout | Badge endpoint initialization |

---

## Category 6: Rate Limiting Stats

### 6A. Drop Reason Recording

**Test:** `rate_limit_drop_records_reason` - [rate_limit.rs:75](node/tests/rate_limit.rs#L75)

**Root Cause:** [node/src/net/peer.rs:1533-1571](node/src/net/peer.rs#L1533-L1571)

**Issue:** `record_drop()` function not incrementing the `DropReason::RateLimit` counter.

**Check:** Ensure the stats HashMap is properly initialized and the drop reason enum matches.

### 6B. Reputation Decrease

**Test:** `reputation_decreases_on_rate_limit` - [rate_limit.rs](node/tests/rate_limit.rs)

**Issue:** Reputation score not decreasing after rate limit drops.

**Check:** The reputation decay function and its trigger conditions.

---

## Hang: snapshot_restore_prop.rs

**Test:** `snapshot_restore_prop` - [snapshot_restore_prop.rs](node/tests/snapshot_restore_prop.rs)

**Symptom:** Test runs indefinitely, logging repeated:
- `storage_engine_policy_enforced`
- `dependency_policy_recorded`
- `no price board found, starting empty`
- `startup_purge`

**Root Cause:** [node/src/config.rs:1015-1067](node/src/config.rs#L1015-L1067)

```rust
runtime::spawn(async move {
    let path = Path::new(&cfg_dir);
    match FsWatcher::new(path, WatchRecursiveMode::NonRecursive) {
        Ok(mut watcher) => loop {      // <-- INFINITE LOOP
            match watcher.next().await {   // <-- Blocks waiting for fs events
                ...
            }
        },
        ...
    }
});
```

**Issue:** Each test iteration (16 from property testing) spawns a new file watcher in an infinite loop. These accumulate and can exhaust system resources.

**Fix Options:**

**Option A: Shutdown watchers properly**
- Add a shutdown channel to the watcher loop
- Call shutdown before dropping Blockchain

**Option B: Reuse single watcher**
- Use a global watcher instead of per-instance

**Option C: Skip watcher in tests**
```rust
if cfg!(test) { return; }
```

---

## Warnings

### Warning 1: Unused Function

**File:** [node/tests/identity_anchor_nonce.rs:57](node/tests/identity_anchor_nonce.rs#L57)

```
warning: function `build_request` is never used
```

**Fix:** Either use the function in tests or remove it.

### Warning 2: Unused Import

**File:** [governance/src/disbursement_auth.rs:11](governance/src/disbursement_auth.rs#L11)

```
warning: unused import: `super::*`
```

**Fix:** Remove the unused import.

---

## Priority Order

**P0 - Critical (blocks all testing):**
1. Fix FeeTooLow issues (affects 8 tests)
2. Fix snapshot hang (blocks test suite completion)

**P1 - High (significant functionality):**
3. Fix chaos test timeouts (core consensus testing)
4. Fix ip_key endianness (rate limiting correctness)

**P2 - Medium (feature-specific):**
5. Fix proximity validation (localnet feature)
6. Fix release flow voting (governance)
7. Fix shard persistence (state management)

**P3 - Low (cleanup):**
8. Fix RPC response parsing (test adaptation)
9. Fix rate limiting stats (metrics)
10. Fix warnings

---

## Quick Wins Checklist

- [ ] Add `bc.min_fee_per_byte_consumer = 0` to 8 mempool tests
- [ ] Change `u32::from(v4)` to `u32::from(v4).swap_bytes()` in gateway.rs:880
- [ ] Add `shard_affinity.sort_by_key(|sa| sa.shard)` in relay.rs:642
- [ ] Add `#[cfg(not(test))]` guard around config watcher spawn
- [ ] Remove unused `build_request` function
- [ ] Remove unused `super::*` import

---

# PART 2: DEEP AUDIT - What the First Analysis Missed

**Audit conducted with fresh perspective - 10x deeper context**

---

## CRITICAL: Silent Failures Not Causing Test Failures (But Should)

### Settlement Persistence Errors (PRODUCTION DATA LOSS RISK)

**Found in logs:** 15+ occurrences of:
```
[ERROR] persist settlement state | value=Custom { kind: Other, error: "io error: No such file or directory (os error 2)" }
```

**Location:** [node/src/compute_market/settlement.rs:285](node/src/compute_market/settlement.rs#L285)

**Impact:** The settlement system is SILENTLY FAILING to persist state. Tests pass because:
1. The error is caught and logged, not propagated
2. The settlement data is in-memory during the test
3. On restart, settlement state is LOST

**This is a PRODUCTION BUG, not just a test issue.** If the settlement directory doesn't exist:
- Compute market settlements are not persisted
- Node restart loses all pending settlements
- Users could lose money

**Root Cause:** The state directory structure isn't created before settlement attempts to write.

**Fix:**
```rust
// In settlement.rs init or persist:
std::fs::create_dir_all(&state_dir)?;
```

### QUIC Certificate Validation Failures (Security Risk)

**Found in logs:**
```
[WARN] QUIC certificate validation failed | peer=723062a7..., error=certificate public key mismatch
```

**Impact:** Peer authentication is FAILING but connections may still proceed. This could allow:
- Man-in-the-middle attacks
- Unauthorized peers joining the network
- Gossip protocol poisoning

**Investigate:** [node/src/p2p/handshake.rs:194](node/src/p2p/handshake.rs#L194)

---

## The Fee System Deep Dive - What I Actually Missed

### The Full Economic Model

The fee system is NOT a simple threshold check. It's a **sophisticated dual-lane economic mechanism**:

**File:** [node/src/fees/lane_pricing.rs](node/src/fees/lane_pricing.rs)

#### Mathematical Model

**Consumer Lane:** `F_c = B_c · C_c(ρ_c) · A_c(t)`
- `B_c`: Base consumer fee (governance parameter)
- `C_c(ρ_c)`: Congestion multiplier from M/M/1 queueing model
- `A_c(t)`: Adaptive adjustment from PI controller

**Industrial Lane:** `F_i = max(B_i · C_i(ρ_i) · M_i(D) · A_i(t), F_c · 1.5)`
- Minimum 50% premium over consumer lane (arbitrage prevention)
- `M_i(D)`: Market demand multiplier using logistic function

#### Congestion Pricing (Queueing Theory)

**File:** [node/src/fees/congestion.rs](node/src/fees/congestion.rs)

Models each lane as M/M/1 queue:
- `ρ = λ/μ` (utilization = arrival rate / service rate)
- Congestion multiplier: `C(ρ) = 1 + k·(ρ/(1-ρ))^n`
- As `ρ → 1`, fees → ∞ (prevents overload)

#### PI Control Theory

**Purpose:** Long-term fee stability
```
A(t+1) = A(t) · (1 + K_p·e(t) + K_i·∫e(t)dt)
```
Where `e(t) = ρ_target - ρ_actual`

#### Why Tests Fail - The Full Picture

1. **Initial state:** `cached_consumer_fee = 1`, `cached_industrial_fee = 2`
2. **Fee update trigger:** Only happens in `mine_block()` at [lib.rs:4997-5004](node/src/lib.rs#L4997-L5004)
3. **Test sequence:**
   - Create blockchain (cached fees = 1, 2)
   - Submit transaction (fee check uses cached value of 1)
   - Transaction rejected if `fee_per_byte < 1`
4. **The sentinel check:** `static_min != 1` means:
   - `0` → use static value 0 (tests want this)
   - `1` → use dynamic cached pricing (default behavior)
   - `2+` → use static value

**The Design Intent:** Value `1` means "use dynamic pricing" because it's the lowest non-zero fee that makes economic sense. Tests should explicitly opt out with `0`.

---

## Global State Pollution - Test Isolation Failures

### Lazy Static Initialization Problem

**File:** [node/src/localnet/proximity.rs:45](node/src/localnet/proximity.rs#L45)

```rust
static TABLE: Lazy<ProximityTable> = Lazy::new(ProximityTable::load);
```

**Problem:** `Lazy` initializes ONCE per process. If:
1. First test's `CARGO_MANIFEST_DIR` resolves incorrectly → empty HashMap
2. All subsequent tests in same process see empty HashMap
3. OR: First test succeeds, second test expects different values → stale data

**Affected Systems Using Lazy Statics:**
- Proximity table
- Config watchers
- Telemetry singletons
- Ban store

**Fix Pattern:**
```rust
#[cfg(test)]
fn reset_for_test() {
    // Allow re-initialization in tests
}
```

### Environment Variable Bleeding

Tests use `std::env::set_var` which affects the ENTIRE process:
- `TB_FAST_MINE`
- `TB_RELEASE_SIGNERS`
- `TB_P2P_SHARD_RATE`
- `TB_NET_KEY_SEED`

If one test sets an env var and another test runs in the same thread/process, the second test sees the first test's values.

**Fix:** Use test-local configuration instead of process-wide env vars, OR reset env vars in test teardown.

---

## The Economics Replay System - Consensus Critical

**File:** [node/src/economics/replay.rs](node/src/economics/replay.rs)

This is **consensus-critical code**. Two nodes seeing the same chain MUST compute identical economics outputs.

### Why It Matters for Tests

The `import_chain()` function calls:
```rust
let replayed_econ = replay_economics_to_tip(&new_chain, &self.params);
```

This replays the ENTIRE economics history for the chain. For a 6-block chain, this:
1. Derives market metrics from on-chain receipts
2. Accumulates ad spend from headers
3. Computes treasury inflow
4. Tracks non-KYC volume
5. Versions governance params at epoch boundaries

**Performance Impact:** Economics replay is O(n) in chain length. For 6 blocks it's fast, but for production chains (millions of blocks), this could take hours.

**Test Implication:** If economics replay diverges, `is_valid_chain_rust()` returns false, and the chain is rejected.

---

## Import Chain Optimization Opportunity

**Current flow at [lib.rs:5617-5817](node/src/lib.rs#L5617-L5817):**

```rust
pub fn import_chain(&mut self, new_chain: Vec<Block>) -> PyResult<()> {
    // 1. Validate entire chain (expensive)
    if !self.is_valid_chain_rust(&new_chain) { return Err(...); }

    // 2. Replay economics (expensive)
    let replayed_econ = replay_economics_to_tip(&new_chain, &self.params);

    // 3. Clear local state
    self.chain.clear();
    self.accounts.clear();

    // 4. Replay ALL blocks one by one
    for block in &new_chain {
        // ... validate each tx
        // ... update accounts
        // ... track receipts
    }
}
```

**Problems:**
1. **Double validation:** `is_valid_chain_rust()` validates, then loop re-validates
2. **No parallelism:** Could validate blocks in parallel
3. **Full replay even for small reorgs:** If LCA is at block 1000 and new chain is 1001, we replay from genesis
4. **Holds mutex entire time:** No concurrent reads during import

**Optimizations:**

1. **Incremental import:** Only replay from LCA
```rust
let lca = find_lca(&self.chain, &new_chain);
if lca > 0 {
    // Only validate blocks after LCA
    for block in &new_chain[lca..] { ... }
}
```

2. **Parallel block validation:**
```rust
use rayon::prelude::*;
let valid = new_chain.par_iter().all(|b| validate_block(b));
```

3. **Copy-on-write state:**
```rust
let mut shadow_state = self.accounts.clone();
// Validate against shadow
// Only swap if valid
std::mem::swap(&mut self.accounts, &mut shadow_state);
```

---

## Architecture Smells Found

### 1. Mixed Blocking/Async Code

**Location:** [node/src/net/mod.rs:1670](node/src/net/mod.rs#L1670)

```rust
if stream.read_to_end(&mut buf).is_ok() {
```

This is BLOCKING I/O in what should be async code. The listener thread can stall indefinitely if a peer:
- Sends partial data
- Doesn't close connection
- Has network issues

**Pattern:** Should use async read with timeout:
```rust
let read_result = timeout(Duration::from_secs(5), async {
    stream.read_to_end(&mut buf).await
}).await;
```

### 2. Coarse-Grained Mutex

**The blockchain mutex** at [net/peer.rs:806](node/src/net/peer.rs#L806) is held for:
- Chain validation
- Economics replay
- Account updates
- Receipt processing
- Difficulty adjustment

This is **the core bottleneck**. Consider:
- Reader-writer lock (RwLock)
- Lock-free data structures for hot paths
- Actor model (message passing instead of shared state)

### 3. No Backpressure

`broadcast_chain()` at [net/mod.rs:1708](node/src/net/mod.rs#L1708) uses fire-and-forget:
```rust
if let Ok(bc) = self.chain.lock() {
    self.broadcast_payload(Payload::Chain(bc.chain.clone()));
}
```

If the lock fails, the broadcast is silently dropped. No retry, no queue, no backpressure.

---

## Test Infrastructure Issues

### 1. Insufficient Test Isolation

Tests share:
- Process environment variables
- Global Lazy statics
- Filesystem (temp dirs can collide)
- Network ports

**Best Practice:** Each test should:
- Create isolated temp dir
- Use unique ports
- Reset global state
- Clean up on exit (even on panic)

### 2. Flaky Timeout Values

**Problem:** Tests use fixed timeouts that work on developer machines but fail on CI:
- `Duration::from_secs(60)` - may timeout on slow CI
- `Duration::from_millis(20)` poll intervals - too aggressive

**Pattern:** Use environment-based timeout scaling:
```rust
fn scaled_timeout(base: Duration) -> Duration {
    let factor = std::env::var("CI_TIMEOUT_FACTOR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    base * factor
}
```

### 3. Property Test Iteration Count

**File:** snapshot_restore_prop.rs uses 16 iterations.

Each iteration spawns file watchers that never shut down. After 16 iterations:
- 16 file watcher threads
- 16 async tasks polling for events
- Resource exhaustion

**Fix:** Either shut down watchers properly OR reduce iteration count for resource-intensive tests.

---

## Network Correctness Concerns

### 1. Rate Limiting Across Architectures

The `ip_key` endianness bug means:
- **x86-64:** IP 1.2.3.4 → key 0x04030201
- **ARM64:** IP 1.2.3.4 → key 0x01020304 (hypothetically)

Same IP gets DIFFERENT rate limit buckets on different architectures. A rate-limited client could bypass limits by connecting to a different-architecture node.

**Impact:** Rate limiting is not architecture-portable.

### 2. Shard Root Persistence

If shard roots aren't persisted correctly:
- Node restart loses shard state hashes
- Shard merkle proofs fail validation
- Light clients can't verify shard state

This is a **data integrity issue** that could cause chain forks.

### 3. Economics Determinism

The economics replay system is marked as "consensus-critical". If ANY of these diverge between nodes:
- Block rewards
- Treasury inflow
- Subsidy allocations
- Tariff calculations

Nodes will compute different chain validity, causing **consensus failure**.

---

## Recommended Fix Priority (Revised)

### P-1 (Emergency - Production Impact)

1. **Settlement directory creation** - Silent data loss in production
2. **QUIC certificate validation** - Security vulnerability

### P0 (Critical - Blocks Development)

3. **Fee system test adaptation** - 8 tests failing
4. **Snapshot hang** - Blocks test suite
5. **Import chain mutex contention** - Performance + test failures

### P1 (High - Functionality)

6. **IP key endianness** - Rate limiting broken across architectures
7. **Shard root persistence** - Data integrity risk
8. **Economics replay optimization** - Performance for long chains

### P2 (Medium - Quality)

9. **Test isolation** - Flaky tests, env var bleeding
10. **Global state resets** - Lazy statics
11. **RPC response format** - API stability

### P3 (Low - Cleanup)

12. **Warnings** - Unused code
13. **Timeout scaling** - CI reliability

---

## Optimization Opportunities Summary

| Area | Current | Optimized | Impact |
|------|---------|-----------|--------|
| Chain import | O(n) full replay | O(k) from LCA | 100x for reorgs |
| Block validation | Sequential | Parallel (rayon) | 4-8x on multi-core |
| Mutex scope | Entire import | State swap only | 10x throughput |
| Economics replay | Per-import | Cached checkpoints | 50x for long chains |
| File watchers | Per-instance | Shared singleton | Eliminates resource leak |
| Fee lookup | Atomic load | Same (already optimal) | N/A |

---

## Test Harness Improvements

### Create `TestBlockchain` Wrapper

```rust
pub struct TestBlockchain {
    bc: Blockchain,
    _dir: TempDir,  // Dropped after bc
}

impl TestBlockchain {
    pub fn new() -> Self {
        let dir = tempdir().unwrap();
        let mut bc = Blockchain::new(dir.path().to_str().unwrap());
        // Disable fee checks for testing
        bc.min_fee_per_byte_consumer = 0;
        bc.min_fee_per_byte_industrial = 0;
        Self { bc, _dir: dir }
    }
}
```

### Create Test Utilities Module

```rust
mod test_utils {
    pub fn with_timeout<F, T>(duration: Duration, f: F) -> T
    where F: FnOnce() -> T {
        // Panic if exceeds timeout
    }

    pub fn reset_global_state() {
        // Clear Lazy statics
        // Reset env vars
    }

    pub fn isolated_port() -> u16 {
        // Return unique port
    }
}
```

---

## Final Checklist - Complete Fix Order

**Day 1 (Emergency):**
- [ ] Fix settlement directory creation - 30 min
- [ ] Audit QUIC certificate validation flow - 2 hours

**Day 2 (Critical):**
- [ ] Add `bc.min_fee_per_byte_consumer = 0` to 8 tests - 1 hour
- [ ] Add `#[cfg(not(test))]` to config watcher - 15 min
- [ ] Fix IP key endianness - 15 min

**Day 3 (Performance):**
- [ ] Reduce mutex scope in import_chain - 4 hours
- [ ] Add incremental import from LCA - 4 hours

**Day 4 (Quality):**
- [ ] Create TestBlockchain wrapper - 2 hours
- [ ] Add test isolation utilities - 2 hours
- [ ] Fix remaining RPC response issues - 2 hours

**Day 5 (Polish):**
- [ ] Remove warnings - 30 min
- [ ] Add CI timeout scaling - 1 hour
- [ ] Documentation review - 2 hours
