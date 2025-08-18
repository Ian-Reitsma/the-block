#!/usr/bin/env bash
# scripts/node_e2e.sh — end-to-end RPC test against local node
#
# Starts a node on a dedicated port, mines to fund Alice, signs a tx,
# submits it, mines it in, and verifies balances.
#
# Usage:
#   ./scripts/node_e2e.sh                  # uses defaults
#   RPC=127.0.0.1:3077 METRICS=127.0.0.1:9177 DATA=node-e2e ./scripts/node_e2e.sh

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

RPC_DEFAULT="127.0.0.1:3050"
METRICS_DEFAULT="127.0.0.1:9150"
DATA_DEFAULT="node-data-e2e"

RPC_ADDR="${RPC:-$RPC_DEFAULT}"
METRICS_ADDR="${METRICS:-$METRICS_DEFAULT}"
DATA_DIR="${DATA:-$DATA_DEFAULT}"

cleanup() {
  if [[ -f /tmp/node-e2e.pid ]]; then
    kill "$(cat /tmp/node-e2e.pid)" 2>/dev/null || true
    rm -f /tmp/node-e2e.pid
  fi
}
trap cleanup EXIT

echo "[e2e] starting node: rpc=$RPC_ADDR metrics=$METRICS_ADDR data=$DATA_DIR"
rm -rf "$DATA_DIR"
RUST_LOG=warn cargo run --features telemetry --bin node -- run \
  --rpc-addr "$RPC_ADDR" \
  --mempool-purge-interval 1 \
  --metrics-addr "$METRICS_ADDR" \
  --data-dir "$DATA_DIR" \
  >/tmp/node-e2e.log 2>&1 & echo $! > /tmp/node-e2e.pid

PID=$(cat /tmp/node-e2e.pid)

# Wait for readiness without requiring ripgrep; also bail if the process dies
READY=0
for i in {1..80}; do
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "[e2e] node process exited early; log follows:" >&2
    tail -n +1 /tmp/node-e2e.log >&2 || true
    exit 1
  fi
  if grep -q "RPC listening on $RPC_ADDR" /tmp/node-e2e.log 2>/dev/null; then
    READY=1; break
  fi
  # Try a lightweight JSON-RPC ping as an alternative readiness check
  if curl -s --max-time 1 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
    --data '{"jsonrpc":"2.0","id":0,"method":"metrics","params":{}}' >/dev/null; then
    READY=1; break
  fi
  sleep 0.25
done
if [[ "$READY" -ne 1 ]]; then
  echo "[e2e] node failed to become ready; log follows:" >&2
  tail -n +1 /tmp/node-e2e.log >&2 || true
  exit 1
fi

echo "[e2e] generating keys"
ADDR_ALICE=$(cargo run --bin node -- generate-key alice_e2e 2>/dev/null | tail -n1)
ADDR_BOB=$(cargo run --bin node -- generate-key bob_e2e 2>/dev/null | tail -n1)
echo "[e2e] ALICE=$ADDR_ALICE"
echo "[e2e] BOB=$ADDR_BOB"

# Lower difficulty for fast demo mining
curl -s --max-time 2 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":0,"method":"set_difficulty","params":{"value":0}}' >/dev/null || true

echo "[e2e] start mining to Alice"
cat >/tmp/rpc-start.json <<JSON
{"jsonrpc":"2.0","id":1,"method":"start_mining","params":{"miner":"$ADDR_ALICE"}}
JSON
curl -s -X POST "$RPC_ADDR" -H 'Content-Type: application/json' --data @/tmp/rpc-start.json | tee /tmp/rpc1.out
sleep 2
curl -s -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":2,"method":"stop_mining","params":{}}' | tee /tmp/rpc2.out

echo "[e2e] check alice balance"
BAL_JSON=$(curl -s -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":3,"method":"balance","params":{"address":"'"$ADDR_ALICE"'"}}')
echo "[e2e] alice balance: $BAL_JSON"

echo "[e2e] signing tx from Alice to Bob"
# ensure Python sees the addresses
export ADDR_ALICE ADDR_BOB
PAYLOAD=$(python - << 'PY'
import json, os
print(json.dumps({
  "from_": os.environ["ADDR_ALICE"],
  "to": os.environ["ADDR_BOB"],
  "amount_consumer": 100,
  "amount_industrial": 0,
  "fee": 200000,
  "fee_selector": 0,
  "nonce": 1,
  "memo": []
}))
PY
)
export ADDR_ALICE ADDR_BOB
TX_HEX=$(cargo run --bin node -- sign-tx alice_e2e "$PAYLOAD" 2>/dev/null | tail -n1)
echo "[e2e] tx bytes=$(( ${#TX_HEX} / 2 ))"

echo "[e2e] submit tx"
curl -s -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":4,"method":"submit_tx","params":{"tx":"'"$TX_HEX"'"}}' | tee /tmp/rpc-submit.out

echo "[e2e] mine to include tx"
cat >/tmp/rpc-start2.json <<JSON
{"jsonrpc":"2.0","id":5,"method":"start_mining","params":{"miner":"$ADDR_ALICE"}}
JSON
curl -s -X POST "$RPC_ADDR" -H 'Content-Type: application/json' --data @/tmp/rpc-start2.json | tee /tmp/rpc3.out
sleep 2
curl -s -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":6,"method":"stop_mining","params":{}}' | tee /tmp/rpc4.out

echo "[e2e] final balances"
BAL_A=$(curl -s -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":7,"method":"balance","params":{"address":"'"$ADDR_ALICE"'"}}')
BAL_B=$(curl -s -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":8,"method":"balance","params":{"address":"'"$ADDR_BOB"'"}}')
echo "[e2e] alice: $BAL_A"
echo "[e2e] bob:   $BAL_B"

# simple pass condition: Bob's consumer >= 100 (parse via Python env var)
CONS=$(JSON="$BAL_B" python - << 'PY'
import os, json
data = json.loads(os.environ['JSON'])
print(data['result']['consumer'])
PY
)
if [[ ${CONS:-0} -lt 100 ]]; then
  echo "[e2e] FAIL: Bob consumer < 100 ($CONS)" >&2
  exit 2
fi
echo "[e2e] PASS: transfer included and balances updated"
