#!/usr/bin/env bash
set -euo pipefail

start=$(date +%s.%N)
fail=0

run_step() {
  local step="$1"
  shift
  if "$@" --prom >/dev/null 2>&1; then
    echo "synthetic_fail_total{step=\"$step\"} 0"
  else
    echo "synthetic_fail_total{step=\"$step\"} 1"
    fail=1
  fi
}

run_step mine cargo run -p probe -- mine-one --timeout 5
run_step gossip cargo run -p probe -- gossip-check --timeout 10
run_step tip cargo run -p probe -- tip --timeout 5

end=$(date +%s.%N)
elapsed=$(echo "$end - $start" | bc)

echo "synthetic_convergence_seconds $elapsed"
if [ "$fail" -eq 0 ]; then
  echo "synthetic_success_total 1"
else
  echo "synthetic_success_total 0"
fi
