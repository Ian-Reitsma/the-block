#!/bin/bash

# Test script for chaos test fixes
# Run this to verify the deadlock fixes work on macOS, Linux, and WSL

set -e

# Portable timeout wrapper: works even if `timeout` is missing
run_with_timeout() {
    local timeout_seconds="$1"; shift
    local cmd=("$@")

    # Prefer GNU coreutils timeout if available (Linux, many macOS setups)
    if command -v timeout >/dev/null 2>&1; then
        # Preserve underlying exit status where possible
        timeout "${timeout_seconds}" "${cmd[@]}"
        return $?
    fi

    # Fallback: pure Bash timer using background watchdog
    # Works on macOS, most Linux, and WSL environments
    local start_pid
    ( "${cmd[@]}" & echo $! >&3 ) 3> >(read start_pid; echo "$start_pid" >&4) 4>tmp_pid_fifo &
    exec 4<tmp_pid_fifo
    read -r start_pid <&4
    rm -f tmp_pid_fifo

    # If for some reason we didn't get a PID, just run without timeout
    if [ -z "$start_pid" ]; then
        "${cmd[@]}"
        return $?
    fi

    # Watchdog: wait for timeout, then kill the process group
    (
        sleep "${timeout_seconds}" || true
        if kill -0 "$start_pid" 2>/dev/null; then
            echo "[timeout] Command exceeded ${timeout_seconds}s: ${cmd[*]}" >&2
            kill "$start_pid" 2>/dev/null || true
            # On some platforms we may need to be more aggressive:
            kill -9 "$start_pid" 2>/dev/null || true
        fi
    ) &
    local watchdog_pid=$!

    # Wait for the command to finish
    wait "$start_pid"
    local cmd_status=$?

    # Stop watchdog if still running
    kill "$watchdog_pid" 2>/dev/null || true

    return "$cmd_status"
}

echo "==============================================" 
echo "Testing Chaos Test Fixes"
echo "Date: $(date)"
echo "Platform: $(uname -s)"
echo "==============================================" 
echo ""

cd /Users/ianreitsma/projects/the-block

echo "Step 1: Verify changes were applied..."
echo "" 

if grep -q "Platform-specific socket tuning" node/tests/chaos.rs; then
    echo "✓ init_env() contains macOS socket tuning"
else
    echo "✗ ERROR: init_env() changes not found"
    exit 1
fi

if grep -q "Give broadcast time to propagate" node/tests/chaos.rs; then
    echo "✓ wait_until_converged() contains deadlock fix"
else
    echo "✗ ERROR: wait_until_converged() changes not found"
    exit 1
fi

if grep -q "CRITICAL: Wait longer for initial peering on Mac" node/tests/chaos.rs; then
    echo "✓ partition_heals_to_majority() contains timing fixes"
else
    echo "✗ ERROR: partition_heals_to_majority() changes not found"
    exit 1
fi

if grep -q "CRITICAL: Let shutdown complete before removing peers" node/tests/chaos.rs; then
    echo "✓ kill_node_recovers() contains timing fixes"
else
    echo "✗ ERROR: kill_node_recovers() changes not found"
    exit 1
fi

echo ""
echo "All changes verified!"
echo ""
echo "==============================================" 
echo "Step 2: Running chaos tests..."
echo "==============================================" 
echo ""

echo "Running converges_under_loss..."
if cargo test --test chaos converges_under_loss --features integration-tests -- --nocapture --test-threads=1 2>&1 | tee /tmp/chaos_test_1.log; then
    echo "✓ converges_under_loss PASSED"
else
    echo "✗ converges_under_loss FAILED"
    tail -50 /tmp/chaos_test_1.log || true
    exit 1
fi

echo ""
echo "Running kill_node_recovers (120s timeout)..."
if run_with_timeout 120 cargo test --test chaos kill_node_recovers --features integration-tests -- --nocapture --test-threads=1 2>&1 | tee /tmp/chaos_test_2.log; then
    echo "✓ kill_node_recovers PASSED"
else
    status=$?
    if [ "$status" -eq 124 ]; then
        echo "✗ kill_node_recovers TIMEOUT (120s limit)"
    else
        echo "✗ kill_node_recovers FAILED (exit code: $status)"
    fi
    tail -50 /tmp/chaos_test_2.log || true
    exit 1
fi

echo ""
echo "Running partition_heals_to_majority (120s timeout)..."
if run_with_timeout 120 cargo test --test chaos partition_heals_to_majority --features integration-tests -- --nocapture --test-threads=1 2>&1 | tee /tmp/chaos_test_3.log; then
    echo "✓ partition_heals_to_majority PASSED"
else
    status=$?
    if [ "$status" -eq 124 ]; then
        echo "✗ partition_heals_to_majority TIMEOUT (120s limit)"
    else
        echo "✗ partition_heals_to_majority FAILED (exit code: $status)"
    fi
    tail -50 /tmp/chaos_test_3.log || true
    exit 1
fi

echo ""
echo "==============================================" 
echo "ALL TESTS PASSED!"
echo "==============================================" 
echo ""
echo "Summary:"
echo "  ✓ converges_under_loss"
echo "  ✓ kill_node_recovers  "
echo "  ✓ partition_heals_to_majority"
echo ""
echo "The deadlock fixes are working!"
echo ""
