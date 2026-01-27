#!/bin/bash
# Verbose test runner with detailed logging and error reporting
#
# Usage: ./run-tests-verbose.sh
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
PACKAGE_COMMANDS_LOG="$LOGS_DIR/package-commands-$TIMESTAMP.txt"

# ANSI color codes for terminal output
RED='\033[0;31m'
YELLOW='\033[1;33m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Handle Ctrl-C gracefully (after color codes are defined)
trap 'echo -e "\n${YELLOW}Test run interrupted by user${NC}"; exit 130' SIGINT SIGTERM

echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  The Block - Comprehensive Test Runner with Verbose Logging${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo ""

# Quick validation test to ensure failure detection works
run_quick_validation() {
    echo -e "${YELLOW}Running quick validation test...${NC}"
    echo ""

    # Create temporary crate directory
    TEMP_DIR="/tmp/test_validation_$$"
    mkdir -p "$TEMP_DIR/src"

    # Create Cargo.toml
    cat > "$TEMP_DIR/Cargo.toml" << 'TEMP_EOF'
[package]
name = "test_validation"
version = "0.1.0"
edition = "2021"

[dependencies]
TEMP_EOF

    # Create test file
    cat > "$TEMP_DIR/src/lib.rs" << 'TEMP_EOF'
#[cfg(test)]
mod quick_validation_tests {
    #[test]
    fn example_passing_test() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn example_second_passing_test() {
        assert_eq!(3 + 3, 6);
    }

    #[test]
    #[ignore]
    fn example_ignored_test() {
        assert_eq!(1 + 1, 2);
    }
}
TEMP_EOF

    # Save current directory and prepare log path
    ORIG_DIR=$(pwd)
    QUICK_LOG="$ORIG_DIR/$LOGS_DIR/quick-validation-$TIMESTAMP.log"

    # Run the test
    cd "$TEMP_DIR"
    cargo test --no-fail-fast -- --test-threads=1 2>&1 | tee "$QUICK_LOG"
    cd "$ORIG_DIR"

    echo ""
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${YELLOW}QUICK VALIDATION TEST RESULTS:${NC}"
    echo ""

    if ! test_result_line=$(grep -m1 "test result:" "$QUICK_LOG"); then
        echo -e "${RED}✗ Quick validation failed - couldn't parse test results${NC}"
        rm -rf "$TEMP_DIR"
        exit 1
    fi

    passed=$(echo "$test_result_line" | sed -E 's/.* ([0-9]+) passed.*/\1/')
    failed=$(echo "$test_result_line" | sed -E 's/.*; ([0-9]+) failed.*/\1/')
    ignored=$(echo "$test_result_line" | sed -E 's/.*; ([0-9]+) ignored.*/\1/')

    [[ -z "$passed" ]] && passed="0"
    [[ -z "$failed" ]] && failed="0"
    [[ -z "$ignored" ]] && ignored="0"

    echo -e "  Passed:  ${GREEN}${passed}${NC}"
    echo -e "  Failed:  ${RED}${failed}${NC}"
    echo -e "  Ignored: ${YELLOW}${ignored}${NC}"
    echo ""

    if [[ "$failed" == "0" && "$passed" != "0" ]]; then
        echo -e "${GREEN}✓ Quick validation passed - failure detection is working!${NC}"
    else
        echo -e "${RED}✗ Quick validation failed - unexpected counts${NC}"
        rm -rf "$TEMP_DIR"
        exit 1
    fi

    echo ""
    echo -e "Quick validation log: ${LOGS_DIR}/quick-validation-${TIMESTAMP}.log"
    echo ""
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    # Cleanup
    rm -rf "$TEMP_DIR"
}

if [[ "${TB_RUN_QUICK_VALIDATION:-0}" == "1" ]]; then
    run_quick_validation
else
    echo -e "${YELLOW}Skipping quick validation (set TB_RUN_QUICK_VALIDATION=1 to enable)${NC}"
    echo ""
fi

echo -e "${GREEN}Test execution started at: $(date)${NC}"
echo -e "${GREEN}Full log: $FULL_LOG${NC}"
echo -e "${GREEN}Error log: $ERROR_LOG${NC}"
echo -e "${GREEN}Failed tests: $FAILED_TESTS_LOG${NC}"
echo -e "${GREEN}Package commands: $PACKAGE_COMMANDS_LOG${NC}"
echo ""

# Initialize error log with header
cat > "$ERROR_LOG" <<EOF
═══════════════════════════════════════════════════════════════
  WARNINGS AND ERRORS REPORT
═══════════════════════════════════════════════════════════════

This file contains all warnings, errors, and test failures found during
the test run, with detailed context to help you understand and fix issues.

Generated at: $(date)

═══════════════════════════════════════════════════════════════
EOF

# Initialize failed tests log
cat > "$FAILED_TESTS_LOG" <<EOF
═══════════════════════════════════════════════════════════════
  COMMANDS TO RE-RUN FAILED TESTS
═══════════════════════════════════════════════════════════════

Generated at: $(date)

To re-run a specific failed test, copy and paste the command below.

═══════════════════════════════════════════════════════════════

EOF

# Initialize package commands log
cat > "$PACKAGE_COMMANDS_LOG" <<EOF
═══════════════════════════════════════════════════════════════
  COMMANDS TO TEST EACH PACKAGE
═══════════════════════════════════════════════════════════════

Generated at: $(date)

To test a specific package, copy and paste the command below.

═══════════════════════════════════════════════════════════════

EOF

# Run complete workspace test with all features
echo -e "${BLUE}Running full workspace test suite with all features...${NC}"
echo -e "${YELLOW}This includes integration tests and all feature combinations${NC}"
echo ""

TOTAL_PASSED=0
TOTAL_FAILED=0
TOTAL_ERRORS=0
FEATURE_GATES_FAILED=0

# Track execution time
START_TIME=$(date +%s)

# Full workspace test command (original comprehensive approach)
TEST_CMD="FIRST_PARTY_ONLY=1 RUST_BACKTRACE=1 cargo test --workspace --all-targets --all-features --no-fail-fast -- --test-threads=1"

# Log the command
echo "" >> "$PACKAGE_COMMANDS_LOG"
echo "# Full Workspace Test (all features, all integration tests)" >> "$PACKAGE_COMMANDS_LOG"
echo "$TEST_CMD" >> "$PACKAGE_COMMANDS_LOG"

# Run the tests with real-time progress tracking
# Track test completion with a progress counter
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Progress tracking function - monitors cargo output for test completion
track_progress() {
    while IFS= read -r line; do
        echo "$line"

        # Count running tests announcements
        if [[ "$line" =~ ^running\ ([0-9]+)\ test ]]; then
            local test_count="${BASH_REMATCH[1]}"
            TESTS_RUN=$((TESTS_RUN + test_count))
        fi

        # Track test results
        if [[ "$line" =~ test\ result:.*([0-9]+)\ passed.*([0-9]+)\ failed ]]; then
            local passed="${BASH_REMATCH[1]}"
            local failed="${BASH_REMATCH[2]}"
            TESTS_PASSED=$((TESTS_PASSED + passed))
            TESTS_FAILED=$((TESTS_FAILED + failed))

            # Show cumulative progress
            local total_completed=$((TESTS_PASSED + TESTS_FAILED))
            echo -e "${CYAN}━━━ Workspace Progress: ${total_completed} tests completed (${TESTS_PASSED} passed, ${TESTS_FAILED} failed) ━━━${NC}" >&2
        fi
    done
}

run_feature_gate() {
    local label="$1"
    local cmd="$2"

    echo "" >> "$PACKAGE_COMMANDS_LOG"
    echo "# Feature gate: $label" >> "$PACKAGE_COMMANDS_LOG"
    echo "$cmd" >> "$PACKAGE_COMMANDS_LOG"

    echo "" >> "$FULL_LOG"
    echo "═══════════════════════════════════════════════════════════════" >> "$FULL_LOG"
    echo "  FEATURE GATE: $label" >> "$FULL_LOG"
    echo "═══════════════════════════════════════════════════════════════" >> "$FULL_LOG"
    echo "$cmd" >> "$FULL_LOG"

    if eval "$cmd" 2>&1 | tee -a "$FULL_LOG"; then
        echo "" >> "$FULL_LOG"
        echo "Feature gate '$label' passed" >> "$FULL_LOG"
    else
        FEATURE_GATES_FAILED=$((FEATURE_GATES_FAILED + 1))
        echo "" >> "$FULL_LOG"
        echo "Feature gate '$label' FAILED" >> "$FULL_LOG"
    fi
}

if eval "$TEST_CMD" 2>&1 | tee "$FULL_LOG" | track_progress; then
    TEST_EXIT_CODE=0
else
    TEST_EXIT_CODE=$?
fi

# Update totals from tracking
TOTAL_PASSED=$TESTS_PASSED
TOTAL_FAILED=$TESTS_FAILED

echo ""
echo -e "${BLUE}Running feature-matrix compile gates...${NC}"
GATE_ENV="FIRST_PARTY_ONLY=1 RUST_BACKTRACE=1"
run_feature_gate "no-default-features" "$GATE_ENV cargo check -p the_block --no-default-features"
run_feature_gate "cli" "$GATE_ENV cargo check -p the_block --features cli"
run_feature_gate "gateway+cli" "$GATE_ENV cargo check -p the_block --features gateway,cli"
run_feature_gate "telemetry+cli" "$GATE_ENV cargo check -p the_block --features telemetry,cli"
if [[ "$FEATURE_GATES_FAILED" -gt 0 ]]; then
    echo ""
    echo -e "${RED}Feature gate checks failed (${FEATURE_GATES_FAILED})${NC}"
    TOTAL_FAILED=$((TOTAL_FAILED + FEATURE_GATES_FAILED))
    TEST_EXIT_CODE=1
else
    echo ""
    echo -e "${GREEN}Feature gate checks passed${NC}"
fi

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
        done
    else
        echo "No test failures found" >> "$error_file"
    fi

    echo "" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"
    echo "  SUMMARY" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"

    # Count issues (remove any newlines and ensure single value)
    error_count=$(grep -c "^error\[E[0-9]*\]:" "$log_file" 2>/dev/null || echo "0")
    error_count=$(echo "$error_count" | head -n1 | tr -d '\n')
    warning_count=$(grep -c "^warning:" "$log_file" 2>/dev/null || echo "0")
    warning_count=$(echo "$warning_count" | head -n1 | tr -d '\n')

    echo "" >> "$error_file"
    echo "Total Compilation Errors: $error_count" >> "$error_file"
    echo "Total Warnings: $warning_count" >> "$error_file"
    echo "Total Test Failures: $TOTAL_FAILED" >> "$error_file"
    echo "Total Tests Passed: $TOTAL_PASSED" >> "$error_file"
    echo "Total Build Errors: $TOTAL_ERRORS" >> "$error_file"
    echo "" >> "$error_file"

    if [[ "${error_count:-0}" -eq 0 && "${TOTAL_FAILED:-0}" -eq 0 && "${TOTAL_ERRORS:-0}" -eq 0 ]]; then
        echo "✓ All tests passed!" >> "$error_file"
    else
        echo "✗ Issues found - please review and fix" >> "$error_file"
    fi

    echo "" >> "$error_file"
    echo "Full log available at: $FULL_LOG" >> "$error_file"
    echo "Failed test commands at: $FAILED_TESTS_LOG" >> "$error_file"
    echo "Package test commands at: $PACKAGE_COMMANDS_LOG" >> "$error_file"
    echo "═══════════════════════════════════════════════════════════════" >> "$error_file"
}

record_failed_test_commands() {
    local log_file="$1"
    local output_file="$2"

    python3 - <<'PY' "$log_file" "$output_file"
import pathlib
import sys

log_path = pathlib.Path(sys.argv[1])
out_path = pathlib.Path(sys.argv[2])
lines = log_path.read_text(errors="ignore").splitlines()
pending_tests = []
commands = []
idx = 0
count = len(lines)

while idx < count:
    line = lines[idx].rstrip("\n")
    if line.strip() == "failures:":
        j = idx + 1
        while j < count and lines[j].strip() == "":
            j += 1
        if j < count and lines[j].startswith("----"):
            idx += 1
            continue
        temp_tests = []
        k = j
        while k < count:
            candidate = lines[k]
            if candidate.strip() == "":
                k += 1
                continue
            if candidate.startswith("    ") or candidate.startswith("\t"):
                temp_tests.append(candidate.strip())
                k += 1
                continue
            break
        pending_tests = temp_tests
        idx = k - 1
    elif line.startswith("error: test failed, to rerun pass '"):
        rerun_target = line.split("pass '", 1)[1].rsplit("'", 1)[0]
        for test_name in pending_tests:
            commands.append((rerun_target, test_name))
        pending_tests = []
    idx += 1

if not commands:
    sys.exit(0)

with out_path.open("a") as out_file:
    out_file.write("\n═══════════════════════════════════════════════════════════════\n")
    out_file.write("  RE-RUN COMMANDS FOR FAILED TESTS\n")
    out_file.write("═══════════════════════════════════════════════════════════════\n\n")
    for target, test in commands:
        out_file.write(f"# {test}\n")
        out_file.write(f"cargo test {target} {test} --all-features -- --nocapture\n\n")
PY
}

echo ""
echo -e "${BLUE}Test execution completed${NC}"
echo ""

# Extract errors and warnings
echo -e "${YELLOW}Analyzing output for errors and warnings...${NC}"
extract_errors "$FULL_LOG" "$ERROR_LOG"
record_failed_test_commands "$FULL_LOG" "$FAILED_TESTS_LOG"

# Display summary
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  LOG FILES CREATED${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "${GREEN}Full log:${NC}           $FULL_LOG"
echo -e "${GREEN}Error log:${NC}          $ERROR_LOG"
echo -e "${GREEN}Failed tests:${NC}       $FAILED_TESTS_LOG"
echo -e "${GREEN}Package commands:${NC}   $PACKAGE_COMMANDS_LOG"
echo ""

# Show quick summary
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  FINAL SUMMARY${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  Total duration:     ${CYAN}${DURATION_STR}${NC}"
echo -e "  Tests passed:       ${GREEN}$TOTAL_PASSED${NC}"
echo -e "  Tests failed:       ${RED}$TOTAL_FAILED${NC}"
if [[ "$FEATURE_GATES_FAILED" -gt 0 ]]; then
    echo -e "  Feature gates:      ${RED}FAILED (${FEATURE_GATES_FAILED})${NC}"
else
    echo -e "  Feature gates:      ${GREEN}PASSED${NC}"
fi
echo ""

if [[ "$TOTAL_FAILED" -eq 0 && "$TEST_EXIT_CODE" -eq 0 ]]; then
    echo -e "${GREEN}✓ All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}✗ Issues found - check logs for details${NC}"
    echo ""
    echo -e "Re-run workspace tests: ${YELLOW}$TEST_CMD${NC}"
    echo -e "Or check ${YELLOW}$FAILED_TESTS_LOG${NC} for specific failed tests"
    exit "$TEST_EXIT_CODE"
fi

echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
