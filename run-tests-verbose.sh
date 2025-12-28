#!/bin/bash
# Verbose test runner with detailed logging and error reporting
#
# Usage: ./run-tests-verbose.sh
#
# Creates two timestamped log files:
# - test-logs/full-TIMESTAMP.log: Complete output
# - test-logs/errors-TIMESTAMP.log: Warnings/errors with context

set -euo pipefail

# Create logs directory
LOGS_DIR="test-logs"
mkdir -p "$LOGS_DIR"

# Generate timestamp for unique filenames
TIMESTAMP=$(date +"%Y%m%d-%H%M%S")

# Log file paths
FULL_LOG="$LOGS_DIR/full-$TIMESTAMP.log"
ERROR_LOG="$LOGS_DIR/errors-$TIMESTAMP.log"

# ANSI color codes for terminal output
RED='\033[0;31m'
YELLOW='\033[1;33m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  The Block - Comprehensive Test Runner with Verbose Logging${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "${GREEN}Test execution started at: $(date)${NC}"
echo -e "${GREEN}Full log: $FULL_LOG${NC}"
echo -e "${GREEN}Error log: $ERROR_LOG${NC}"
echo ""

# Initialize error log with header
cat > "$ERROR_LOG" << 'EOF'
═══════════════════════════════════════════════════════════════
  WARNINGS AND ERRORS REPORT
═══════════════════════════════════════════════════════════════

This file contains all warnings, errors, and test failures found during
the test run, with detailed context to help you understand and fix issues.

Generated at: $(date)
Command: FIRST_PARTY_ONLY=1 RUST_BACKTRACE=FULL cargo test --workspace --all-targets --all-features -- --test-threads=1

═══════════════════════════════════════════════════════════════
EOF

# Replace $(date) with actual date
sed -i '' "s/\$(date)/$(date)/" "$ERROR_LOG"

# Function to extract and format errors from log
extract_errors() {
    local log_file="$1"
    local error_file="$2"

    echo "" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"
    echo "  COMPILATION ERRORS" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"

    # Extract compilation errors with context
    grep -n "^error\[E[0-9]*\]:" "$log_file" | while IFS=: read -r line_num error_line; do
        echo "" >> "$error_file"
        echo "───────────────────────────────────────────────────────────────" >> "$error_file"
        echo "ERROR at line $line_num in log:" >> "$error_file"
        echo "───────────────────────────────────────────────────────────────" >> "$error_file"

        # Get 10 lines of context (5 before, the error, 4 after)
        sed -n "$((line_num - 5)),$((line_num + 4))p" "$log_file" >> "$error_file"

        echo "" >> "$error_file"
        echo "HOW TO FIX:" >> "$error_file"

        # Provide specific guidance based on error type
        case "$error_line" in
            *"unresolved import"*)
                echo "  - This is a missing dependency or import issue" >> "$error_file"
                echo "  - Check the file path shown above" >> "$error_file"
                echo "  - Verify the dependency is listed in Cargo.toml" >> "$error_file"
                echo "  - For dev dependencies (tests/benches), add to [dev-dependencies]" >> "$error_file"
                ;;
            *"cannot find"*)
                echo "  - This is a missing type, function, or module" >> "$error_file"
                echo "  - Check spelling and ensure the item exists" >> "$error_file"
                echo "  - Verify imports at the top of the file" >> "$error_file"
                ;;
            *"mismatched types"*)
                echo "  - Type mismatch between expected and actual types" >> "$error_file"
                echo "  - Check the function signature or variable declaration" >> "$error_file"
                echo "  - May need type conversion or different method" >> "$error_file"
                ;;
            *)
                echo "  - See error details above" >> "$error_file"
                echo "  - Check the file and line number indicated" >> "$error_file"
                echo "  - Run 'rustc --explain E####' for more info (replace #### with error code)" >> "$error_file"
                ;;
        esac

        echo "" >> "$error_file"
    done || echo "No compilation errors found" >> "$error_file"

    echo "" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"
    echo "  WARNINGS" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"

    # Extract warnings with context
    grep -n "^warning:" "$log_file" | while IFS=: read -r line_num warning_line; do
        echo "" >> "$error_file"
        echo "───────────────────────────────────────────────────────────────" >> "$error_file"
        echo "WARNING at line $line_num in log:" >> "$error_file"
        echo "───────────────────────────────────────────────────────────────" >> "$error_file"

        # Get context
        sed -n "$((line_num - 2)),$((line_num + 3))p" "$log_file" >> "$error_file"

        echo "" >> "$error_file"
        echo "RECOMMENDATION:" >> "$error_file"

        case "$warning_line" in
            *"unused"*)
                echo "  - Remove the unused item or prefix with underscore (_item)" >> "$error_file"
                echo "  - Or use #[allow(dead_code)] if intentionally unused" >> "$error_file"
                ;;
            *"deprecated"*)
                echo "  - Update to use the recommended replacement" >> "$error_file"
                echo "  - Check documentation for migration guide" >> "$error_file"
                ;;
            *)
                echo "  - See warning details above" >> "$error_file"
                echo "  - Consider fixing to keep codebase clean" >> "$error_file"
                ;;
        esac

        echo "" >> "$error_file"
    done || echo "No warnings found" >> "$error_file"

    echo "" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"
    echo "  TEST FAILURES" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"

    # Extract test failures with context
    # Look for failures section and extract failed test names
    if grep -q "^failures:" "$log_file"; then
        awk '/^failures:/{flag=1; next} /^test result:/{flag=0} flag && /^[[:space:]]+[a-zA-Z_]/ {print}' "$log_file" | while read -r test_name; do
            test_name=$(echo "$test_name" | xargs)  # trim whitespace
            [[ -z "$test_name" ]] && continue

            echo "" >> "$error_file"
            echo "───────────────────────────────────────────────────────────────" >> "$error_file"
            echo "FAILED TEST: $test_name" >> "$error_file"
            echo "───────────────────────────────────────────────────────────────" >> "$error_file"
            echo "" >> "$error_file"

            # Find the failure details in the failures section
            echo "FAILURE DETAILS:" >> "$error_file"
            # Look for the test name in the failures section
            grep -A 20 "^---- $test_name stdout ----" "$log_file" >> "$error_file" 2>/dev/null || echo "  (Details not found in log)" >> "$error_file"

            echo "" >> "$error_file"
            echo "HOW TO DEBUG:" >> "$error_file"
            echo "  1. Run the specific test: cargo test $test_name -- --nocapture" >> "$error_file"
            echo "  2. Check the assertion or panic message above" >> "$error_file"
            echo "  3. Add println! debugging to see intermediate values" >> "$error_file"
            echo "  4. Check if test fixtures or data need updating" >> "$error_file"
            echo "" >> "$error_file"
        done
    else
        echo "No test failures found" >> "$error_file"
    fi

    echo "" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"
    echo "  SUMMARY" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"

    # Count issues
    error_count=$(grep -c "^error\[E[0-9]*\]:" "$log_file" || echo "0")
    warning_count=$(grep -c "^warning:" "$log_file" || echo "0")
    # Extract failure count from "test result:" line (e.g., "test result: FAILED. 0 passed; 1 failed")
    failure_count=$(grep "^test result:" "$log_file" | sed -n 's/.*; \([0-9]\+\) failed.*/\1/p' || echo "0")
    [[ -z "$failure_count" ]] && failure_count="0"

    echo "" >> "$error_file"
    echo "Total Errors:   $error_count" >> "$error_file"
    echo "Total Warnings: $warning_count" >> "$error_file"
    echo "Total Failures: $failure_count" >> "$error_file"
    echo "" >> "$error_file"

    if [[ "$error_count" -eq 0 && "$failure_count" -eq 0 ]]; then
        echo "✓ All tests passed!" >> "$error_file"
    else
        echo "✗ Issues found - please review and fix" >> "$error_file"
    fi

    echo "" >> "$error_file"
    echo "Full log available at: $FULL_LOG" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"
}

# Run tests and capture all output
echo -e "${BLUE}Running tests...${NC}"
echo ""

# Run the test command, capturing both stdout and stderr
if FIRST_PARTY_ONLY=1 RUST_BACKTRACE=full cargo test --workspace --all-targets --all-features -- --test-threads=1 2>&1 | tee "$FULL_LOG"; then
    TEST_EXIT_CODE=0
else
    TEST_EXIT_CODE=$?
fi

echo ""
echo -e "${BLUE}Test execution completed${NC}"
echo ""

# Extract errors and warnings
echo -e "${YELLOW}Analyzing output for errors and warnings...${NC}"
extract_errors "$FULL_LOG" "$ERROR_LOG"

# Display summary
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  LOG FILES CREATED${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "${GREEN}Full log:${NC}    $FULL_LOG"
echo -e "${GREEN}Error log:${NC}   $ERROR_LOG"
echo ""

# Show quick summary from error log
if [[ -f "$ERROR_LOG" ]]; then
    error_count=$(grep "^Total Errors:" "$ERROR_LOG" | awk '{print $3}' || echo "0")
    warning_count=$(grep "^Total Warnings:" "$ERROR_LOG" | awk '{print $3}' || echo "0")
    failure_count=$(grep "^Total Failures:" "$ERROR_LOG" | awk '{print $3}' || echo "0")

    [[ -z "$error_count" ]] && error_count="0"
    [[ -z "$warning_count" ]] && warning_count="0"
    [[ -z "$failure_count" ]] && failure_count="0"

    echo -e "${BLUE}SUMMARY:${NC}"
    echo -e "  Errors:   ${RED}$error_count${NC}"
    echo -e "  Warnings: ${YELLOW}$warning_count${NC}"
    echo -e "  Failures: ${RED}$failure_count${NC}"
    echo ""

    if [[ "${error_count:-0}" -eq 0 && "${failure_count:-0}" -eq 0 ]]; then
        echo -e "${GREEN}✓ All tests passed!${NC}"
        if [[ "${warning_count:-0}" -gt 0 ]]; then
            echo -e "${YELLOW}  (But there are $warning_count warnings to review)${NC}"
        fi
    else
        echo -e "${RED}✗ Issues found - check $ERROR_LOG for details${NC}"
    fi
fi

echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"

# Exit with the same code as the test command
exit $TEST_EXIT_CODE
