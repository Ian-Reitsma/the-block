# The Block Test Commands Reference

## Full Crate Test with All Features

### Command for Complete Test Suite:
```bash
cd /Users/ianreitsma/projects/the-block

# Run ALL tests with ALL features (recommended for comprehensive testing)
FIRST_PARTY_ONLY=1 RUST_BACKTRACE=full cargo test --workspace --all-targets --all-features --no-fail-fast -- --test-threads=1
```

### What This Does:
- `--workspace`: Tests all crates in the workspace
- `--all-targets`: Tests lib, bins, examples, tests, and benches
- `--all-features`: Enables ALL feature flags (integration-tests, telemetry, quic, etc.)
- `--no-fail-fast`: Continue testing even after failures (see all problems)
- `--test-threads=1`: Run tests serially (important for integration tests)
- `FIRST_PARTY_ONLY=1`: Use only first-party dependencies  
- `RUST_BACKTRACE=full`: Get full stack traces on failures

### Chaos-Specific Tests:
```bash
# Run just chaos tests with verbose output
cargo test --test chaos --features integration-tests -- --nocapture --test-threads=1

# Run a specific chaos test
cargo test --test chaos partition_heals_to_majority --features integration-tests -- --nocapture --test-threads=1

# With timeout (120 seconds)
timeout 120 cargo test --test chaos partition_heals_to_majority --features integration-tests -- --nocapture --test-threads=1
```

### Quick Tests (No Integration):
```bash
# Fast unit tests only (skip slow integration tests)
cargo test --workspace --lib --all-features
```

### Per-Package Testing:
```bash
# Test specific package
cargo test -p the_block --all-features
cargo test -p ledger --all-features
cargo test -p crypto_suite --all-features
```

## Using run-tests-verbose.sh

The `run-tests-verbose.sh` script already runs the full comprehensive test suite:

```bash
chmod +x run-tests-verbose.sh
./run-tests-verbose.sh
```

This script:
- Runs the full command above automatically
- Creates detailed logs in `test-logs/`
- Extracts errors, warnings, and failures
- Provides commands to re-run failed tests
- Gives you a summary at the end

## Diagnostic Testing

For detailed chaos test diagnostics:

```bash
chmod +x run-chaos-tests-diagnostic.sh
./run-chaos-tests-diagnostic.sh
```

This will:
- Run all 3 chaos tests individually
- NOT exit on first failure (runs all to see the pattern)
- Show detailed output for each test
- Save logs to /tmp/chaos_test_*.log
- Give you a summary of passed/failed/timeout

## Interpreting Results

### Test Passes:
```
test result: ok. X passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Test Failures:
```
test result: FAILED. X passed; Y failed; 0 ignored; 0 measured; 0 filtered out
```

### Timeout (120s):
Indicates the test is hanging - likely a blockchain convergence deadlock or infinite loop

### What Each Log Shows:

**Full Log** (`test-logs/full-*.log`):
- Complete output of all tests
- Compilation messages
- Test execution details
- Final summary

**Error Log** (`test-logs/errors-*.log`):
- Extracted compilation errors with context
- Warnings
- Test failure details
- How to fix suggestions

**Failed Tests Log** (`test-logs/failed-tests-*.log`):
- Exact commands to re-run failed tests
- Copy-paste ready

## Common Issues

### "error[E0609]: no field `X`"
You're trying to access a private field. Use public API methods instead.

### "test timed out after 120 seconds"
This is a REAL PROBLEM - the blockchain convergence is hanging. Not a test issue.

### "test result: FAILED. ... panicked at ..."
Test assertion failed. Read the panic message to see what went wrong.

## Performance Notes

### Fast (~1-5 minutes):
- `cargo test --lib` - Unit tests only
- `cargo test -p specific_crate` - Single package

### Medium (~10-20 minutes):
- `cargo test --workspace` - All packages, no integration

### Slow (~30+ minutes):
- `cargo test --workspace --all-features` - Everything
- Integration tests with network simulation
- Chaos tests (partition, recovery, convergence)

## Debugging Failed Tests

### Step 1: Identify the failure
```bash
# Check the error log
cat test-logs/errors-TIMESTAMP.log
```

### Step 2: Run the specific test with full output
```bash
# Copy command from failed-tests log
cargo test --test chaos partition_heals_to_majority --features integration-tests -- --nocapture --test-threads=1
```

### Step 3: Look for convergence issues
```bash
# Search the log for diagnostic output
grep "CONVERGENCE" /tmp/chaos_test_3.log
grep "Node.*height" /tmp/chaos_test_3.log
grep "FAILED" /tmp/chaos_test_3.log
```

### Step 4: Check if it's a timing issue or real bug
- **Timing issue**: Nodes converge but test times out
  - Solution: Increase timeout or add more delays
- **Real bug**: Nodes never converge (heights stay different)
  - Solution: Fix blockchain sync/broadcast logic

## Next Steps After Running Tests

### If Tests Pass:
✓ Blockchain convergence is working
✓ Deadlock fixes are effective
✓ Ready to commit changes

### If Tests Fail:
1. Read the error log to see exact failure
2. Check if it's compilation error or runtime failure
3. Look at node heights in diagnostic output
4. Check peer counts to verify network topology
5. If heights diverge, there's a consensus bug
6. If heights match but test times out, increase timeout

### If Tests Timeout:
1. This means blockchain is NOT converging
2. Check broadcast_chain() implementation
3. Check chain synchronization logic
4. Look for deadlocks in peer management
5. Verify fork choice rule is deterministic

## The Key Question

**Are nodes converging to the same chain?**

- **YES**: Test timing issue - adjust delays/timeouts
- **NO**: Blockchain consensus bug - fix sync logic

You can tell by looking at the final diagnostic output:
```
Node 0: height=5, peers=4
Node 1: height=5, peers=4  
Node 2: height=5, peers=4
Node 3: height=3, peers=2  <-- NOT CONVERGED
Node 4: height=5, peers=4
```

If heights match → test issue
If heights differ → consensus bug
