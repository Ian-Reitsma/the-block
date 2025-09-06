#!/usr/bin/env bash
# scripts/node_drive_existing.sh — drive an already-running node via JSON-RPC
#
# Uses the node CLI to generate keys and sign a transaction, and curl to
# start/stop mining, submit the tx, and verify balances.
#
# Usage:
#   RPC=127.0.0.1:3030 ./scripts/node_drive_existing.sh

set -euo pipefail

RPC_ADDR="${RPC:-127.0.0.1:3030}"
echo "[drive] using RPC=$RPC_ADDR"

# Make mining fast for demos: set difficulty to 0
curl -s --max-time 5 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":0,"method":"set_difficulty","params":{"value":0}}' >/dev/null || true

ADDR_ALICE=$(cargo run --bin node -- generate-key alice_cli 2>/dev/null | tail -n1)
ADDR_BOB=$(cargo run --bin node -- generate-key bob_cli 2>/dev/null | tail -n1)
echo "[drive] ALICE=$ADDR_ALICE"
echo "[drive] BOB=$ADDR_BOB"

echo "[drive] start mining"
curl -s --max-time 5 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"start_mining","params":{"miner":"'"$ADDR_ALICE"'"}}'
echo
sleep 2
curl -s --max-time 5 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":2,"method":"stop_mining","params":{}}'
echo

# give the mining loop time to complete the in-flight block before we try to lock for balance
sleep 3

echo "[drive] alice balance"
curl -s --max-time 15 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":3,"method":"balance","params":{"address":"'"$ADDR_ALICE"'"}}'
echo

echo "[drive] sign tx"
# ensure Python sees the addresses
export ADDR_ALICE ADDR_BOB
PAYLOAD=$(python - << 'PY'
import json, os
print(json.dumps({
  "from_": os.environ["ADDR_ALICE"],
  "to": os.environ["ADDR_BOB"],
  "amount_consumer": 100,
  "amount_industrial": 0,
  "fee": 300000,
  "pct_ct": 0,
  "nonce": 1,
  "memo": []
}))
PY
)
TX_HEX=$(ADDR_ALICE="$ADDR_ALICE" ADDR_BOB="$ADDR_BOB" cargo run --bin node -- sign-tx alice_cli "$PAYLOAD" 2>/dev/null | tail -n1)
echo "[drive] tx bytes=$(( ${#TX_HEX} / 2 ))"

echo "[drive] submit tx"
curl -s --max-time 5 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":4,"method":"submit_tx","params":{"tx":"'"$TX_HEX"'"}}'
echo

echo "[drive] mine inclusion"
curl -s --max-time 5 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":5,"method":"start_mining","params":{"miner":"'"$ADDR_ALICE"'"}}'
echo
sleep 2
curl -s --max-time 5 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":6,"method":"stop_mining","params":{}}'
echo

# wait for the in-flight block to finish so balances query doesn't block
sleep 3

echo "[drive] final balances"
printf 'Alice: '
curl -s --max-time 15 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":7,"method":"balance","params":{"address":"'"$ADDR_ALICE"'"}}'
echo
printf 'Bob:   '
curl -s --max-time 15 -X POST "$RPC_ADDR" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":8,"method":"balance","params":{"address":"'"$ADDR_BOB"'"}}'
echo
