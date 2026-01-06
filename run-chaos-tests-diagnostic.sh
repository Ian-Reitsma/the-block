#!/bin/bash

# Diagnostic chaos test runner - shows exactly what's happening
# Does NOT exit on first failure - runs all tests to see the pattern

echo "================================================================================"
echo "CHAOS TEST DIAGNOSTIC RUN"
echo "Date: $(date)"
echo "Platform: $(uname -s)"
echo "================================================================================"
echo ""

cd /Users/ianreitsma/projects/the-block

PASSED=0
FAILED=0
TIMEOUT=0

echo "================================================================================"
echo "TEST 1: converges_under_loss"
echo "================================================================================"
if timeout 120 cargo test --test chaos converges_under_loss --features integration-tests -- --nocapture --test-threads=1 2>&1 | tee /tmp/chaos_test_1.log; then
    RESULT="PASSED"
    PASSED=$((PASSED + 1))
    echo ""
    echo "✓ converges_under_loss PASSED"
else
    EXITCODE=$?
    if [ $EXITCODE -eq 124 ]; then
        RESULT="TIMEOUT"
        TIMEOUT=$((TIMEOUT + 1))
        echo ""
        echo "✗ converges_under_loss TIMEOUT (120s)"
    else
        RESULT="FAILED"
        FAILED=$((FAILED + 1))
        echo ""
        echo "✗ converges_under_loss FAILED (exit code: $EXITCODE)"
    fi
    echo ""
    echo "Last 30 lines of output:"
    tail -30 /tmp/chaos_test_1.log
fi
echo ""
echo "================================================================================"
echo ""
sleep 2

echo "================================================================================"
echo "TEST 2: kill_node_recovers"
echo "================================================================================"
if timeout 120 cargo test --test chaos kill_node_recovers --features integration-tests -- --nocapture --test-threads=1 2>&1 | tee /tmp/chaos_test_2.log; then
    RESULT="PASSED"
    PASSED=$((PASSED + 1))
    echo ""
    echo "✓ kill_node_recovers PASSED"
else
    EXITCODE=$?
    if [ $EXITCODE -eq 124 ]; then
        RESULT="TIMEOUT"
        TIMEOUT=$((TIMEOUT + 1))
        echo ""
        echo "✗ kill_node_recovers TIMEOUT (120s)"
    else
        RESULT="FAILED"
        FAILED=$((FAILED + 1))
        echo ""
        echo "✗ kill_node_recovers FAILED (exit code: $EXITCODE)"
    fi
    echo ""
    echo "Last 30 lines of output:"
    tail -30 /tmp/chaos_test_2.log
fi
echo ""
echo "================================================================================"
echo ""
sleep 2

echo "================================================================================"
echo "TEST 3: partition_heals_to_majority"
echo "================================================================================"
if timeout 120 cargo test --test chaos partition_heals_to_majority --features integration-tests -- --nocapture --test-threads=1 2>&1 | tee /tmp/chaos_test_3.log; then
    RESULT="PASSED"
    PASSED=$((PASSED + 1))
    echo ""
    echo "✓ partition_heals_to_majority PASSED"
else
    EXITCODE=$?
    if [ $EXITCODE -eq 124 ]; then
        RESULT="TIMEOUT"
        TIMEOUT=$((TIMEOUT + 1))
        echo ""
        echo "✗ partition_heals_to_majority TIMEOUT (120s)"
    else
        RESULT="FAILED"
        FAILED=$((FAILED + 1))
        echo ""
        echo "✗ partition_heals_to_majority FAILED (exit code: $EXITCODE)"
    fi
    echo ""
    echo "Last 30 lines of output:"
    tail -30 /tmp/chaos_test_3.log
fi
echo ""

echo "================================================================================"
echo "SUMMARY"
echo "================================================================================"
echo "Passed:  $PASSED / 3"
echo "Failed:  $FAILED / 3"
echo "Timeout: $TIMEOUT / 3"
echo ""

if [ $PASSED -eq 3 ]; then
    echo "✓ ALL TESTS PASSED!"
    echo ""
    echo "The blockchain convergence is working correctly."
    exit 0
else
    echo "✗ SOME TESTS FAILED"
    echo ""
    echo "This indicates blockchain convergence issues, not just test problems."
    echo ""
    echo "Full logs saved to:"
    echo "  /tmp/chaos_test_1.log"
    echo "  /tmp/chaos_test_2.log"
    echo "  /tmp/chaos_test_3.log"
    echo ""
    echo "To investigate:"
    echo "  1. Check logs for 'CONVERGENCE TIMEOUT' messages"
    echo "  2. Look for node height mismatches"
    echo "  3. Check peer counts"
    echo ""
    exit 1
fi
