# Testing Guide

## Verbose Test Runner

The project includes a comprehensive test runner with detailed logging and error reporting.

### Quick Start

```bash
./run-tests-verbose.sh
```

This script will:
- Run the full test suite with `FIRST_PARTY_ONLY=1` and full backtraces
- Create timestamped log files in `test-logs/`
- Generate a detailed error report with context and fix suggestions
- Display results in the terminal with colored output

### Log Files

Two log files are created for each test run:

#### 1. Full Log: `test-logs/full-YYYYMMDD-HHMMSS.log`
Contains the complete output of the test run, including:
- All compilation output
- Test execution output
- Backtraces
- Timing information

#### 2. Error Log: `test-logs/errors-YYYYMMDD-HHMMSS.log`
A curated report containing:
- **Compilation Errors** - With context lines and fix suggestions
- **Warnings** - With recommendations
- **Test Failures** - With failure details and debugging tips
- **Summary** - Count of errors, warnings, and failures

### Error Log Format

Each error/warning in the error log includes:

1. **Location** - Line number in the full log
2. **Context** - 5-10 lines of surrounding code/output
3. **How to Fix** - Specific guidance based on error type
4. **Commands** - Exact commands to run for debugging

Example:
```
───────────────────────────────────────────────────────────────
ERROR at line 1234 in log:
───────────────────────────────────────────────────────────────
error[E0432]: unresolved import `testkit`
 --> crypto/benches/dilithium.rs:1:5
  |
1 | use testkit::tb_bench;
  |     ^^^^^^^ use of unresolved module or unlinked crate `testkit`

HOW TO FIX:
  - This is a missing dependency or import issue
  - Check the file path shown above
  - Verify the dependency is listed in Cargo.toml
  - For dev dependencies (tests/benches), add to [dev-dependencies]
```

### Manual Test Commands

If you prefer to run tests manually:

```bash
# Full test suite with logging
FIRST_PARTY_ONLY=1 RUST_BACKTRACE=full cargo test --workspace --all-targets --all-features -- --test-threads=1

# Specific test with output
cargo test test_name -- --nocapture

# Run only unit tests
cargo test --lib

# Run only integration tests
cargo test --test '*'

# Run specific crate tests
cargo test -p the_block

# Check without running
cargo check --workspace --all-targets --all-features
```

### Tips

1. **Review Error Log First** - Start with `errors-*.log` for quick issue overview
2. **Use Full Log for Context** - Reference full log when you need more details
3. **Timestamped Files** - Old logs are preserved, so you can compare runs
4. **Colored Output** - Terminal output uses colors to highlight issues
5. **Exit Code** - Script exits with test command's exit code (0 = success)

### Cleaning Up Logs

```bash
# Remove old logs (keep last 10)
cd test-logs && ls -t | tail -n +11 | xargs rm -f

# Remove all logs
rm -rf test-logs/
```

## Test Organization

- **Unit tests** - In `src/` files with `#[cfg(test)]` modules
- **Integration tests** - In `node/tests/` directory
- **Benchmarks** - In `*/benches/` directories
- **Examples** - In `examples/` directory

## Test Isolation Patterns

- **Disable periodic chain pulls**: set `TB_P2P_CHAIN_SYNC_INTERVAL_MS=0` in isolation tests so background sync ticks do not mask ordering bugs.
- **Timeout scaling**: use `TB_TEST_TIMEOUT_MULT` to widen timeouts under slow CI, keep it near `1` locally.
- **Fast mining**: `TB_FAST_MINE=1` speeds deterministic fixtures; pair with smaller reactor backoffs when timing matters.
- **Rate-limit shaping**: adjust `TB_P2P_RATE_WINDOW_SECS`, `TB_P2P_MAX_PER_SEC`, and `TB_P2P_MAX_BYTES_PER_SEC` for deterministic throttling tests.

## CI/CD

The same test configuration is used in CI:
- All warnings are treated as potential issues
- Tests run single-threaded for determinism
- Full backtraces enabled for debugging
