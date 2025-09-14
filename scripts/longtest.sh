#!/usr/bin/env bash
set -euo pipefail

duration=${1:-43200} # 12 hours default
end=$((SECONDS + duration))
metrics_file=${LONGTEST_METRICS:-longtest-metrics.prom}
:> "$metrics_file"

while [ $SECONDS -lt $end ]; do
  cargo test --all --features test-telemetry --release
  curl -sf localhost:9898/metrics >> "$metrics_file" 2>/dev/null || true
  echo "---" >> "$metrics_file"
  sleep 1
done
