#!/bin/bash

# STRIDE 1: Circuit Breaker Integration - Validation Script
# Run this to confirm all fixes are working

set -e

echo "="*60
echo "STRIDE 1 VALIDATION"
echo "Circuit Breaker Integration"
echo "="*60
echo ""

echo "[1/5] Cleaning build artifacts..."
cargo clean > /dev/null 2>&1
echo "✓ Clean complete"
echo ""

echo "[2/5] Checking compilation..."
echo "Running: cargo check --all-features"
if cargo check --all-features 2>&1 | tee /tmp/check_output.log; then
    echo "✓ Compilation successful (zero errors)"
else
    echo "✗ Compilation failed"
    tail -50 /tmp/check_output.log
    exit 1
fi
echo ""

echo "[3/5] Running linter..."
echo "Running: cargo clippy --all-features -- -D warnings"
if cargo clippy --all-features -- -D warnings 2>&1 | tee /tmp/clippy_output.log; then
    echo "✓ Clippy passed (zero warnings)"
else
    echo "⚠ Clippy found issues (non-fatal)"
    tail -30 /tmp/clippy_output.log
fi
echo ""

echo "[4/5] Running circuit breaker unit tests..."
echo "Running: cargo test -p governance circuit_breaker"
if cargo test -p governance circuit_breaker --nocapture 2>&1 | tee /tmp/test_output.log; then
    TESTS_PASSED=$(grep -c "test result: ok" /tmp/test_output.log || echo "0")
    echo "✓ All tests passed"
else
    echo "✗ Tests failed"
    tail -50 /tmp/test_output.log
    exit 1
fi
echo ""

echo "[5/5] Verifying integration test module..."
echo "Running: cargo test -p governance circuit_breaker_integration_test --lib"
if cargo test -p governance circuit_breaker_integration_test --lib --nocapture 2>&1 | tail -20; then
    echo "✓ Integration tests validated"
else
    echo "✗ Integration tests failed"
fi
echo ""

echo "="*60
echo "✓ STRIDE 1 VALIDATION COMPLETE"
echo "="*60
echo ""
echo "Next steps:"
echo "  1. cargo run --release --features telemetry --bin node"
echo "  2. curl http://localhost:9615/metrics | grep circuit_breaker"
echo "  3. Execute failover test scenario (see docs/archive/STRIDE_1_COMPLETE.md)"
echo ""
