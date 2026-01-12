#!/usr/bin/env bash
set -euo pipefail

# Helper to launch a node with sane defaults for a 3-node LAN cluster.
# Usage: ROLE=primary ./scripts/multi-node/run-node.sh
# Roles: primary, replica1, observer

ROLE="${ROLE:-primary}"
DATA_ROOT="${DATA_ROOT:-$HOME/.the_block/multi-node}"
FEATURES="${FEATURES:-telemetry,quic}"

case "$ROLE" in
  primary)
    RPC_ADDR="${RPC_ADDR:-0.0.0.0:3030}"
    STATUS_ADDR="${STATUS_ADDR:-0.0.0.0:3031}"
    METRICS_ADDR="${METRICS_ADDR:-0.0.0.0:9898}"
    QUIC_PORT="${QUIC_PORT:-9000}"
    ;;
  replica1)
    RPC_ADDR="${RPC_ADDR:-0.0.0.0:4030}"
    STATUS_ADDR="${STATUS_ADDR:-0.0.0.0:4031}"
    METRICS_ADDR="${METRICS_ADDR:-0.0.0.0:9899}"
    QUIC_PORT="${QUIC_PORT:-9001}"
    ;;
  observer)
    RPC_ADDR="${RPC_ADDR:-0.0.0.0:5030}"
    STATUS_ADDR="${STATUS_ADDR:-0.0.0.0:5031}"
    METRICS_ADDR="${METRICS_ADDR:-0.0.0.0:9900}"
    QUIC_PORT="${QUIC_PORT:-9002}"
    ;;
  *)
    echo "unknown ROLE='$ROLE' (expected primary|replica1|observer)" >&2
    exit 1
    ;;
esac

DATA_DIR="${DATA_ROOT}/${ROLE}"
DB_PATH="${DB_PATH:-$DATA_DIR/db}"
LOG_LEVEL="${LOG_LEVEL:-info}"
LOG_FORMAT="${LOG_FORMAT:-plain}"

mkdir -p "$DATA_DIR"

# Rate limits and energy RPC throttles stay bounded for LAN testing.
export TB_RPC_TOKENS_PER_SEC="${TB_RPC_TOKENS_PER_SEC:-256}"
export TB_RPC_ENERGY_TOKENS_PER_SEC="${TB_RPC_ENERGY_TOKENS_PER_SEC:-96}"
export TB_RPC_BAN_SECS="${TB_RPC_BAN_SECS:-30}"
export TB_RPC_CLIENT_TIMEOUT_SECS="${TB_RPC_CLIENT_TIMEOUT_SECS:-120}"

# P2P shaping to keep local meshes predictable.
export TB_P2P_MAX_PER_SEC="${TB_P2P_MAX_PER_SEC:-500}"
export TB_P2P_MAX_BYTES_PER_SEC="${TB_P2P_MAX_BYTES_PER_SEC:-1048576}"
export TB_P2P_RATE_WINDOW_SECS="${TB_P2P_RATE_WINDOW_SECS:-1}"

# Enable range-boost discovery on LAN so the three nodes can find each other without hardcoded seeds.
RANGE_BOOST_FLAG="--range-boost"

cmd=(
  cargo run -p the_block --features "$FEATURES" --bin node --
  run
  --rpc-addr "$RPC_ADDR"
  --status-addr "$STATUS_ADDR"
  --metrics-addr "$METRICS_ADDR"
  --data-dir "$DATA_DIR"
  --db-path "$DB_PATH"
  --log-level "$LOG_LEVEL"
  --log-format "$LOG_FORMAT"
  --quic
  --quic-port "$QUIC_PORT"
  $RANGE_BOOST_FLAG
)

echo "Launching node role=$ROLE data_dir=$DATA_DIR rpc=$RPC_ADDR metrics=$METRICS_ADDR quic=$QUIC_PORT"
exec "${cmd[@]}"
