# STRIDE 1: QUICK START VALIDATION (5 MINUTES)

## Step 1: Verify Compilation (2 min)

```bash
cd /Users/ianreitsma/projects/the-block

# Clean build
cargo clean

# Check compilation
cargo check --all-features

# Expected: "Finished check" with zero errors
```

**If you see errors:** Stop and check STRIDE_1_ALL_FIXES.md - 90% of issues are import-related

---

## Step 2: Run Tests (2 min)

```bash
# Run circuit breaker unit tests
cargo test -p governance circuit_breaker --nocapture

# Expected: "test result: ok. 10 passed"
```

**If tests fail:** Check test output - likely cause is CircuitBreaker not imported in test module

---

## Step 3: Validate Telemetry (1 min)

```bash
# Verify function exists
grep -n "set_circuit_breaker_state" node/src/telemetry/treasury.rs

# Expected: Function definition + #[cfg] gates
```

---

## All Tests Passed? ✅

You're done with STRIDE 1!

### Next: Manual Verification

```bash
# Start node with telemetry
TB_GOVERNANCE_DB_PATH=/tmp/gov_db cargo run --release --features telemetry --bin node 2>&1 | grep circuit

# In separate terminal:
curl http://localhost:9615/metrics 2>/dev/null | grep circuit_breaker

# Expected output:
# treasury_circuit_breaker_state 0.0
# treasury_circuit_breaker_failures 0.0
# treasury_circuit_breaker_successes 0.0
```

---

## Troubleshooting

### "unresolved imports CircuitBreaker"
**Fix:** Run `cargo clean && cargo check --all-features`

### "no field named circuit_breaker"
**Fix:** Verify `node/src/governance/store.rs` has the new fields - check STRIDE_1_ALL_FIXES.md

### Tests hang or timeout
**Fix:** Check test has `#[test]` attribute and uses `std::thread::sleep`

---

## Documentation Files

- **STRIDE_1_COMPLETE.md** - Full implementation guide (comprehensive)
- **STRIDE_1_ARCHITECTURE.md** - System design diagrams (detailed)
- **STRIDE_1_ALL_FIXES.md** - Every code change (reference)
- **validate_stride_1.sh** - Automated validation script

---

## Success Criteria

✅ `cargo check --all-features` = 0 errors  
✅ `cargo test circuit_breaker` = 10 passed  
✅ `/metrics` endpoint shows 3 gauges  
✅ No warnings from clippy

If all ✅, **STRIDE 1 is COMPLETE**.

