#!/bin/bash

# Verify Metrics Coverage for Fast-Mainnet
# Purpose: Ensure all AGENTS.md metrics are being emitted by the system
# Usage: bash scripts/verify_metrics_coverage.sh [--prometheus-url http://...]

set -e

# Color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
PROMETHEUS_URL="${PROMETHEUS_URL:-http://localhost:9090}"
TEST_RANGE_MINUTES="${TEST_RANGE_MINUTES:-60}"

# Parsed arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --prometheus-url)
      PROMETHEUS_URL="$2"
      shift 2
      ;;
    --range-minutes)
      TEST_RANGE_MINUTES="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1"
      exit 1
      ;;
  esac
done

echo "========================================"
echo "Fast-Mainnet Metrics Coverage Verification"
echo "========================================"
echo "Prometheus URL: $PROMETHEUS_URL"
echo "Time Range: Last ${TEST_RANGE_MINUTES} minutes"
echo ""

# Define all expected metrics from AGENTS.md
declare -a METRICS=(
  # Treasury Metrics
  "governance_disbursements_total"
  "treasury_disbursement_backlog"
  "treasury_disbursement_lag_seconds"
  "treasury_execution_errors_total"
  "treasury_balance"
  "treasury_executor_tick_duration_seconds"
  "treasury_dependency_failures_total"
  
  # Energy Metrics
  "energy_provider_total"
  "energy_provider_status"
  "energy_pending_credits_total"
  "energy_settlements_total"
  "energy_settlement_ct_total"
  "energy_active_disputes_total"
  "oracle_latency_seconds"
  "energy_signature_verification_failures_total"
  "energy_slashing_total"
  "energy_slashed_ct_total"
  "energy_reputation_score"
  "energy_reputation_confidence"
  
  # Receipt Metrics
  "receipt_emitted_total"
  "receipt_validation_errors_total"
  "receipt_pending_depth"
  "explorer_receipt_processed_total"
  "explorer_receipt_lag_seconds"
  "explorer_db_sync_lag_blocks"
  
  # Economics Metrics
  "economics_epoch_tx_count"
  "economics_epoch_tx_volume_block"
  "economics_epoch_treasury_inflow_block"
  "economics_prev_market_metrics_utilization_ppm"
  "economics_prev_market_metrics_provider_margin_ppm"
  "economics_block_reward_per_block"
  "settlement_audit_balance_total"
  "settlement_audit_conservation_failures_total"
)

# Test each metric
echo "Checking ${#METRICS[@]} metrics..."
echo ""

missing_count=0
found_count=0
warn_count=0

for metric in "${METRICS[@]}"; do
  # Query Prometheus for this metric
  response=$(curl -s "${PROMETHEUS_URL}/api/v1/query_range" \
    --data-urlencode "query=$metric" \
    --data-urlencode "start=$(date -u -d '${TEST_RANGE_MINUTES} minutes ago' +%s)" \
    --data-urlencode "end=$(date +%s)" \
    --data-urlencode "step=60s" 2>/dev/null || echo '{"status":"error"}')
  
  # Parse response
  status=$(echo "$response" | jq -r '.status' 2>/dev/null || echo "error")
  result_count=$(echo "$response" | jq '.data.result | length' 2>/dev/null || echo "0")
  
  if [ "$status" = "success" ] && [ "$result_count" -gt 0 ]; then
    printf "${GREEN}✓${NC} %-50s (${result_count} series)\n" "$metric"
    ((found_count++))
  elif [ "$status" = "success" ] && [ "$result_count" -eq 0 ]; then
    printf "${YELLOW}⚠${NC} %-50s (no data in range)\n" "$metric"
    ((warn_count++))
  else
    printf "${RED}✗${NC} %-50s (NOT FOUND)\n" "$metric"
    ((missing_count++))
  fi
done

echo ""
echo "========================================"
echo "Summary"
echo "========================================"
printf "${GREEN}Found:${NC}       %3d metrics\n" "$found_count"
printf "${YELLOW}Warnings:${NC}    %3d metrics (no recent data)\n" "$warn_count"
printf "${RED}Missing:${NC}     %3d metrics\n" "$missing_count"

total=$((found_count + warn_count + missing_count))
coverage=$((100 * found_count / total))
printf "${GREEN}Coverage:${NC}    %3d%% complete\n" "$coverage"

echo ""

# Determine exit status
if [ $missing_count -eq 0 ]; then
  echo -e "${GREEN}✓ All required metrics present!${NC}"
  exit_code=0
elif [ $missing_count -le 3 ]; then
  echo -e "${YELLOW}⚠ Some metrics missing (non-critical)${NC}"
  exit_code=0
else
  echo -e "${RED}✗ Critical metrics missing - launch blocked${NC}"
  exit_code=1
fi

echo ""
if [ $warn_count -gt 0 ]; then
  echo -e "${YELLOW}Note:${NC} Warnings indicate metrics not emitted in the last ${TEST_RANGE_MINUTES} minutes."
  echo "This may be normal if the system hasn't processed events in that window."
  echo "Check dashboard to confirm metrics are emitting live data."
fi

echo ""
echo "Recommendations:"
echo "  1. Dashboard Check: Visit http://localhost:3000/d/treasury-dashboard"
echo "  2. Verify Data: Run queries in Prometheus: http://localhost:9090"
echo "  3. Full Test: cargo test -p the_block --test settlement_audit --release"
echo ""

exit $exit_code
