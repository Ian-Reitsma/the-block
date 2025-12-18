# Transport Handshake & Test Isolation Fixes - Dev Documentation

## Overview

This document details fixes for four interconnected test failures that surfaced after previous changes to the inhouse transport layer. The root causes were: a socket contention deadlock, global state pollution in parallel tests, a timing-dependent benchmark test, and missing test isolation guards.

---

## Issue 1: Transport `handshake_success_roundtrip` Deadlock

### File: `crates/transport/src/inhouse/adapter.rs`

### Root Cause

The `Connection` struct shares a single `UdpSocket` between `send()` and `client_receiver_loop()` via `Arc<AsyncMutex<Option<UdpSocket>>>`. Both operations use `.take()` to temporarily own the socket.

**The deadlock sequence:**

1. `Connection::from_handshake()` spawns `client_receiver_loop` as a background task
2. If the executor immediately runs `client_receiver_loop`, it calls `recv_datagram()`
3. `recv_datagram()` takes the socket via `.take()` and calls `udp.recv_from().await`
4. `recv_from()` blocks **indefinitely** waiting for incoming data
5. Meanwhile, the test calls `send()` which needs the socket
6. `send()` finds socket is `None` (recv has it), retries via `yield_now()`
7. **Deadlock**: recv waits for data, send can't send data because recv has the socket

### The Fix

Modified `recv_datagram()` to use a **timeout** on `recv_from()`. This ensures the socket is periodically released, giving `send()` a window to acquire it.

```rust
const RECV_YIELD_INTERVAL: Duration = Duration::from_millis(50);

async fn recv_datagram(
    socket: &Arc<AsyncMutex<Option<UdpSocket>>>,
    shutdown: &CancellationToken,
) -> std::io::Result<(Vec<u8>, SocketAddr)> {
    let mut buf = vec![0u8; MAX_DATAGRAM];
    loop {
        if shutdown.is_cancelled() {
            return Err(IoError::new(ErrorKind::NotConnected, "connection closed"));
        }
        // Acquire socket with retry
        let mut udp = loop {
            {
                let mut guard = socket.lock().await;
                if let Some(s) = guard.take() {
                    break s;
                }
            }
            runtime::yield_now().await;
            if shutdown.is_cancelled() {
                return Err(IoError::new(ErrorKind::NotConnected, "connection closed"));
            }
        };
        // Timeout ensures we periodically release socket for send()
        let result = runtime::timeout(RECV_YIELD_INTERVAL, udp.recv_from(&mut buf)).await;
        // CRITICAL: Always return socket before processing result
        {
            let mut guard = socket.lock().await;
            *guard = Some(udp);
        }
        match result {
            Ok(Ok((len, addr))) => {
                buf.truncate(len);
                return Ok((buf, addr));
            }
            Ok(Err(e)) => return Err(e),
            Err(_timeout) => continue, // Release socket, loop again
        }
    }
}
```

### Key Insight

The **order of operations matters**:
1. Take socket
2. Do I/O with timeout
3. **Return socket** (before processing result or returning)
4. Then process result

This pattern ensures the socket is always returned even on timeout or error.

### What to Watch For

- If you see "connection closed before ack" or timeout errors in inhouse transport tests, check:
  - Is `recv_datagram` holding the socket too long?
  - Is the timeout value appropriate for the test environment?
  - On slow CI systems, you may need to increase `RECV_YIELD_INTERVAL`

---

## Issue 2: metrics-aggregator Test Failures

### Files: `metrics-aggregator/src/lib.rs`

### Root Cause: Two Separate Issues

#### 2a. Missing Test Guard

Test `tls_warning_status_reports_counts_and_retention` was missing `metrics_registry_guard()`:

```rust
// BEFORE (broken):
#[test]
fn tls_warning_status_reports_counts_and_retention() {
    install_tls_env_warning_forwarder();  // No guard!
    reset_tls_warning_snapshots();
    // ... sets retention to 15
}

// AFTER (fixed):
#[test]
fn tls_warning_status_reports_counts_and_retention() {
    let _guard = metrics_registry_guard();  // Serializes with other tests
    install_tls_env_warning_forwarder();
    reset_tls_warning_snapshots();
    // ...
}
```

Without the guard, this test runs in parallel and corrupts `TLS_WARNING_RETENTION_SECS`, causing other tests to see wrong retention values.

#### 2b. Global State Pollution from TLS Warning Forwarder

The TLS warning forwarder installs a global sink that captures **all** TLS warnings across the entire test run. When tests trigger real TLS warnings (via `server_tls_from_env()`), these get recorded in `TLS_WARNING_SNAPSHOTS`.

Test `tls_warning_retention_override_applies` was counting all snapshots:
```rust
// BEFORE (broken):
let snapshots = tls_warning_snapshots();
assert_eq!(snapshots.len(), 1);  // Fails if other tests added snapshots

// AFTER (fixed):
let snapshots: Vec<_> = tls_warning_snapshots()
    .into_iter()
    .filter(|s| s.prefix == "TB_OVERRIDE")
    .collect();
assert_eq!(snapshots.len(), 1);  // Only counts our test's entries
```

### The Pattern: Test Isolation for Global State

When tests manipulate global state (atomics, lazy statics, global registries), use this pattern:

```rust
// 1. Create a guard function
fn metrics_registry_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

// 2. Acquire guard at start of EVERY test that touches shared state
#[test]
fn my_test() {
    let _guard = metrics_registry_guard();
    reset_global_state();
    // ... test logic
}
```

### What to Watch For

- If metrics-aggregator tests fail with unexpected counts or values:
  - Check if all tests using shared state have the guard
  - Check if tests filter results by their unique prefix
  - Look for log output showing TLS warnings from other tests

---

## Issue 3: testkit `regression_thresholds_trigger_alert_files`

### File: `crates/testkit/tests/bench.rs`

### Root Cause

The test used an empty benchmark body with threshold `per_iter=0`:

```rust
bench::run("threshold_regression", 1, || {});  // Empty body = 0ns execution
```

With 0ns per iteration, the regression check `per_iter > 0` evaluates to `false` (0 > 0 is false), so no alert is written.

### The Fix

Add actual work so the benchmark exceeds the threshold:

```rust
bench::run("threshold_regression", 1, || {
    std::thread::sleep(std::time::Duration::from_millis(1));
});
```

Now `per_iter` is ~1ms, which is > 0, triggering the alert.

### What to Watch For

- Benchmark threshold tests must ensure the benchmark **actually exceeds** the threshold
- Empty bodies may execute in 0ns on fast systems
- When writing threshold tests, use either:
  - A body with actual work (sleep, computation)
  - A very small threshold that any non-zero work will exceed (e.g., `0.000000001`)

---

## Issue 4: TLS Warning Metrics Test Isolation

### File: `node/tests/tls_warning_metrics.rs`

### Root Cause

Four tests were manipulating global TLS warning state without synchronization:
- `tls_env_warning_metrics_increment_on_log`
- `tls_env_warning_metrics_from_diagnostics_without_sink`
- `tls_env_warning_telemetry_sink_captures_forwarder_events`
- `tls_env_warning_telemetry_sink_captures_diagnostics_bridge_events`

When run in parallel:
1. Test A calls `clear_tls_warning_sinks_for_testing()`
2. Test B calls `register_tls_warning_sink()`
3. Test A asserts `!has_tls_warning_sinks()` - **FAILS** (Test B added one)

### The Fix

Added a test guard identical to the metrics-aggregator pattern:

```rust
fn tls_warning_test_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

#[test]
fn tls_env_warning_metrics_from_diagnostics_without_sink() {
    let _guard = tls_warning_test_guard();  // Added to all 4 tests
    http_env::clear_tls_warning_sinks_for_testing();
    // ...
}
```

### Global State Inventory

These globals require test serialization when modified:

| Crate | Global | Purpose |
|-------|--------|---------|
| `http_env` | `TLS_WARNING_SINKS` | TLS warning callbacks |
| `metrics-aggregator` | `TLS_WARNING_SNAPSHOTS` | Warning history |
| `metrics-aggregator` | `TLS_WARNING_RETENTION_SECS` | Pruning retention |
| `node/telemetry` | `TLS_WARNING_SUBSCRIBER` | Diagnostics bridge |
| `node/telemetry` | `TLS_ENV_WARNINGS` | Per-node warning state |

---

## Diagnostic Checklist

When you see test failures similar to these, check:

### For Deadlocks/Timeouts in Transport
1. Is a blocking operation holding a shared resource?
2. Does the operation have a timeout that releases the resource?
3. Is there a circular dependency between operations?

### For "Wrong Value" Assertions
1. Is global state being modified?
2. Are all tests using a serialization guard?
3. Are tests filtering results by unique identifiers?

### For "File Not Found" in Tests
1. Does the operation that creates the file actually trigger?
2. Are thresholds set correctly for the test's workload?
3. Is the parent directory being created?

### For "Unexpected Count" Assertions
1. Is there a global forwarder/sink capturing events?
2. Are parallel tests triggering the same event types?
3. Should the test filter by a unique prefix/identifier?

---

## Testing the Fixes

Run all previously failing tests:

```bash
# Transport tests (takes ~60s due to handshake timeouts)
cargo test -p transport --test inhouse

# Metrics-aggregator TLS warning tests
cargo test -p metrics-aggregator --lib -- tls_warning

# Testkit regression threshold test
cargo test -p testkit --test bench regression_thresholds

# TLS warning metrics (requires telemetry feature)
cargo test -p the_block --test tls_warning_metrics --features telemetry
```

---

## Files Modified

| File | Change |
|------|--------|
| `crates/transport/src/inhouse/adapter.rs` | Added timeout to `recv_datagram()` to prevent deadlock |
| `metrics-aggregator/src/lib.rs` | Added missing guard, filtered snapshots by prefix |
| `crates/testkit/tests/bench.rs` | Added sleep to benchmark body |
| `node/tests/tls_warning_metrics.rs` | Added test guard to all 4 tests |
| `cli/src/logs.rs` | Changed `once_cell` import to `foundation_lazy` |
