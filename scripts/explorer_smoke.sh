#!/usr/bin/env bash
set -euo pipefail

RPC_PORT=3070
P2P_PORT=3071
METRICS_PORT=9898
TMP=$(mktemp -d)

# start node in background
./target/release/node --datadir "$TMP/datadir" --config "$TMP/config.toml" \
  --rpc-port $RPC_PORT --p2p-port $P2P_PORT --metrics-port $METRICS_PORT &
NODE_PID=$!
trap "kill $NODE_PID" EXIT

# wait for RPC to come up
for _ in {1..20}; do
  if curl -sSf "http://127.0.0.1:$RPC_PORT/health" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

curl -sSf "http://127.0.0.1:$RPC_PORT/health" >/dev/null
curl -sSf "http://127.0.0.1:$RPC_PORT/tip" >/dev/null
curl -sSf "http://127.0.0.1:$RPC_PORT/peers" >/dev/null
