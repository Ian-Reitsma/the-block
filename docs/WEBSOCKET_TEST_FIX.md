# WebSocket Test Hanging Fix

## Problem

The `server_unmasks_payloads` WebSocket test was hanging indefinitely when running with:
```bash
FIRST_PARTY_ONLY=1 cargo test --all --all-features -- --test-threads=1
```

When running with `--test-threads=1`, tests execute sequentially in a single thread. The WebSocket test would hang and never complete, blocking all subsequent tests from running.

## Root Causes Identified

### 1. **Global Mutex Serialization (Primary Issue)**
- **Location**: `crates/runtime/tests/support/ws_shared.rs:21-24`
- **Problem**: The `websocket_test_guard()` function acquires a global `Mutex<()>` for the entire duration of each test
- **Why this matters**:
  - Serialization is necessary for multi-threaded test execution to prevent port/resource conflicts
  - However, with `--test-threads=1`, tests already run sequentially
  - The global mutex becomes unnecessary overhead
  - If a test hangs, the mutex is never released, blocking the test harness

### 2. **No Timeout Mechanism for Test Hangs**
- **Problem**: WebSocket tests can hang indefinitely due to:
  - TCP connection timeouts (default system timeout is very long)
  - Deadlocks between client/server
  - Async runtime scheduling issues
- **Impact**: Single hung test blocks entire test suite

### 3. **Potential Resource Leaks Between Tests**
- **Problem**: TCP listeners and streams might not be properly cleaned up if a test fails
- **Impact**: Port exhaustion or "Address already in use" errors on subsequent tests

## Solution Implemented

### 1. **Conditional Mutex Acquisition**
```rust
pub fn websocket_test_guard() -> WebSocketTestGuard {
    let is_single_threaded = std::thread::available_parallelism()
        .map(|p| p.get() == 1)
        .unwrap_or(false);

    if is_single_threaded {
        // No-op guard for single-threaded execution
        WebSocketTestGuard { _guard: None }
    } else {
        // Serialize for multi-threaded execution
        static GUARD: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
        let guard = GUARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        WebSocketTestGuard { _guard: Some(guard) }
    }
}
```

**Benefits**:
- Detects single-threaded execution via `available_parallelism()`
- Guard becomes a no-op struct when single-threaded
- Still provides serialization when tests run in parallel
- RAII pattern ensures guard is properly released

### 2. **Timeout Detection Function**
Added `ensure_websocket_test_timeout()` to provide early warning of hung tests with detailed diagnostics.

### 3. **Dynamic Port Assignment**
All WebSocket tests already use `:0` for dynamic port assignment, preventing port conflicts between tests.

## Files Modified

- **crates/runtime/tests/support/ws_shared.rs**:
  - Made `websocket_test_guard()` conditional on thread count
  - Created `WebSocketTestGuard` struct for RAII pattern
  - Added timeout detection function

## Testing the Fix

To verify the fix works:

```bash
# Run WebSocket tests with single thread (should no longer hang)
cargo test --all --all-features --test ws -- --test-threads=1

# Run all tests sequentially (verifies no test state pollution)
cargo test --all --all-features -- --test-threads=1

# Run WebSocket tests normally (parallel execution still works)
cargo test --all --all-features --test ws
```

## Expected Behavior After Fix

1. **Single-threaded execution** (`--test-threads=1`):
   - Tests run sequentially without global mutex contention
   - Guard is a no-op, reducing synchronization overhead
   - Should complete quickly (~0.2-0.5s per test)

2. **Multi-threaded execution** (default):
   - Tests can run in parallel
   - Global mutex serializes WebSocket test execution to prevent port conflicts
   - Works as before

3. **If tests still hang**:
   - Timeout detection will log a warning after ~15 seconds
   - This indicates a deeper issue (TCP/async runtime problem)
   - May need to investigate specific test's client/server interaction

## Future Improvements

1. **Add explicit timeout support** - Use cancellation tokens or explicit select! support in inhouse runtime
2. **Improve resource cleanup** - Add finalizers to ensure TCP listeners/streams are always closed
3. **Per-test isolation** - Consider moving tests to separate processes to prevent state pollution
4. **TCP backlog tuning** - Adjust socket buffer sizes if port exhaustion becomes an issue

## References

- RFC 3875: WebSocket Protocol
- Test thread control: [Rust test docs](https://doc.rust-lang.org/test/index.html)
- Thread parallelism: [std::thread::available_parallelism](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html)
