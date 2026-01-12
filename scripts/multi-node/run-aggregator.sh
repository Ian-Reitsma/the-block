#!/usr/bin/env bash
set -euo pipefail

# Minimal launcher for the first-party metrics aggregator.
# Intended to run on the observer Mac in the 3-node LAN testbed.

DATA_ROOT="${DATA_ROOT:-$HOME/.the_block/multi-node}"
DB_PATH="${DB_PATH:-$DATA_ROOT/aggregator.db}"
ADDR="${ADDR:-0.0.0.0:9000}"
TOKEN="${TOKEN:-local-dev-token}"
RETENTION="${RETENTION:-604800}" # 7d

mkdir -p "$DATA_ROOT"

export AGGREGATOR_ADDR="$ADDR"
export AGGREGATOR_DB="$DB_PATH"
export AGGREGATOR_TOKEN="$TOKEN"
export AGGREGATOR_RETENTION_SECS="$RETENTION"

echo "Starting metrics-aggregator on $ADDR (db=$DB_PATH)"
exec cargo run -p metrics-aggregator --
