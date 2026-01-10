#!/bin/bash
# Test runner with minimal output (non-verbose version)
#
# Usage: ./run-tests.sh
#
# Creates timestamped log files:
# - test-logs/full-TIMESTAMP.log: Complete output
# - test-logs/errors-TIMESTAMP.log: Warnings/errors with context
# - test-logs/failed-tests-TIMESTAMP.txt: Commands to re-run failed tests

set -euo pipefail

# Create logs directory
LOGS_DIR="test-logs"
mkdir -p "$LOGS_DIR"

# Generate timestamp for unique filenames
TIMESTAMP=$(date +"%Y%m%d-%H%M%S")

# Log file paths
FULL_LOG="$LOGS_DIR/full-$TIMESTAMP.log"
ERROR_LOG="$LOGS_DIR/errors-$TIMESTAMP.log"
FAILED_TESTS_LOG="$LOGS_DIR/failed-tests-$TIMESTAMP.txt"

# ANSI color codes for terminal output
RED='\033[0;31m'
YELLOW='\033[1;33m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Handle Ctrl-C gracefully
trap 'echo -e "\n${YELLOW}Test run interrupted by user${NC}"; exit 130' SIGINT SIGTERM

echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  The Block - Test Runner${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo ""

echo -e "${GREEN}Test execution started at: $(date)${NC}"
echo -e "${GREEN}Full log: $FULL_LOG${NC}"
echo -e "${GREEN}Error log: $ERROR_LOG${NC}"
echo ""

# Initialize error log with header
cat > "$ERROR_LOG" <<EOF
═══════════════════════════════════════════════════════════════
  WARNINGS AND ERRORS REPORT
═══════════════════════════════════════════════════════════════

Generated at: $(date)

═══════════════════════════════════════════════════════════════
EOF

# Initialize failed tests log
cat > "$FAILED_TESTS_LOG" <<EOF
═══════════════════════════════════════════════════════════════
  COMMANDS TO RE-RUN FAILED TESTS
═══════════════════════════════════════════════════════════════

Generated at: $(date)

═══════════════════════════════════════════════════════════════

EOF

# Run complete workspace test with all features
echo -e "${BLUE}Running full workspace test suite with all features...${NC}"
echo ""

TOTAL_PASSED=0
TOTAL_FAILED=0

# Track execution time
START_TIME=$(date +%s)

# Full workspace test command - quieter version:
# - RUST_BACKTRACE=1 instead of full (shorter backtraces)
# - RUST_LOG=error to suppress INFO/WARN messages from tests
TEST_CMD="FIRST_PARTY_ONLY=1 RUST_BACKTRACE=1 RUST_LOG=error cargo test --workspace --all-targets --all-features --no-fail-fast -- --test-threads=1"

# Track test results
TESTS_PASSED=0
TESTS_FAILED=0

# Progress tracking function - shows only test results, not INFO logs
track_progress() {
    local current_binary=""
    while IFS= read -r line; do
        # Log everything to file
        echo "$line" >> "$FULL_LOG"

        # Only show relevant output to terminal (filter out INFO logs)
        case "$line" in
            *"Compiling"*|*"Finished"*|*"Running"*)
                echo "$line"
                ;;
            *"running "*" test"*)
                echo "$line"
                ;;
            *"test result:"*)
                echo "$line"
                # Parse results
                if [[ "$line" =~ ([0-9]+)\ passed.*([0-9]+)\ failed ]]; then
                    local passed="${BASH_REMATCH[1]}"
                    local failed="${BASH_REMATCH[2]}"
                    TESTS_PASSED=$((TESTS_PASSED + passed))
                    TESTS_FAILED=$((TESTS_FAILED + failed))
                fi
                ;;
            *"FAILED"*|*"error"*|*"panicked"*)
                # Always show failures
                echo -e "${RED}$line${NC}"
                ;;
            *"test "*.." ok"*)
                # Optionally show passing tests (comment out for even quieter)
                # echo "$line"
                :
                ;;
        esac
    done
}

echo -e "${CYAN}Running tests (INFO logs suppressed, see $FULL_LOG for details)...${NC}"
echo ""

if eval "$TEST_CMD" 2>&1 | track_progress; then
    TEST_EXIT_CODE=0
else
    TEST_EXIT_CODE=$?
fi

# Update totals from tracking
TOTAL_PASSED=$TESTS_PASSED
TOTAL_FAILED=$TESTS_FAILED

END_TIME=$(date +%s)
TOTAL_DURATION=$((END_TIME - START_TIME))

# Format duration
DURATION_STR="${TOTAL_DURATION}s"
if [[ $TOTAL_DURATION -ge 60 ]]; then
    MINS=$((TOTAL_DURATION / 60))
    SECS=$((TOTAL_DURATION % 60))
    DURATION_STR="${MINS}m ${SECS}s"
fi

echo ""
echo -e "${BLUE}Test suite completed in ${DURATION_STR}${NC}"
echo ""

# Function to extract errors from log
extract_errors() {
    local log_file="$1"
    local error_file="$2"

    echo "" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"
    echo "  COMPILATION ERRORS" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"

    # Extract compilation errors
    grep -n "^error\[E[0-9]*\]:" "$log_file" >> "$error_file" 2>/dev/null || echo "No compilation errors found" >> "$error_file"

    echo "" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"
    echo "  TEST FAILURES" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"

    # Extract test failures
    if grep -q "^failures:" "$log_file"; then
        awk '/^failures:/{flag=1; next} /^test result:/{flag=0} flag && /^[[:space:]]+[a-zA-Z_]/ {print}' "$log_file" >> "$error_file"
    else
        echo "No test failures found" >> "$error_file"
    fi

    echo "" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"
    echo "  SUMMARY" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"

    # Count issues
    error_count=$(grep -c "^error\[E[0-9]*\]:" "$log_file" 2>/dev/null || echo "0")
    error_count=$(echo "$error_count" | head -n1 | tr -d '\n')

    echo "" >> "$error_file"
    echo "Total Compilation Errors: $error_count" >> "$error_file"
    echo "Total Test Failures: $TOTAL_FAILED" >> "$error_file"
    echo "Total Tests Passed: $TOTAL_PASSED" >> "$error_file"
    echo "" >> "$error_file"

    if [[ "${error_count:-0}" -eq 0 && "${TOTAL_FAILED:-0}" -eq 0 ]]; then
        echo "All tests passed!" >> "$error_file"
    else
        echo "Issues found - please review and fix" >> "$error_file"
    fi
}

# Extract errors and warnings
echo -e "${YELLOW}Analyzing output...${NC}"
extract_errors "$FULL_LOG" "$ERROR_LOG"

# Display summary
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  FINAL SUMMARY${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  Total duration:     ${CYAN}${DURATION_STR}${NC}"
echo -e "  Tests passed:       ${GREEN}$TOTAL_PASSED${NC}"
echo -e "  Tests failed:       ${RED}$TOTAL_FAILED${NC}"
echo ""
echo -e "  Full log:           ${GREEN}$FULL_LOG${NC}"
echo -e "  Error log:          ${GREEN}$ERROR_LOG${NC}"
echo ""

if [[ "$TOTAL_FAILED" -eq 0 && "$TEST_EXIT_CODE" -eq 0 ]]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Issues found - check logs for details${NC}"
    exit "$TEST_EXIT_CODE"
fi
