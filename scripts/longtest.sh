#!/usr/bin/env bash
set -euo pipefail

duration=${1:-43200} # 12 hours default
end=$((SECONDS + duration))

while [ $SECONDS -lt $end ]; do
  cargo test --all --features test-telemetry --release
  sleep 1
done
