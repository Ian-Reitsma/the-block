#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${REPO_ROOT}/target/release/node"
CHAIN_ID="worldos-energy"
NODE_NAME=${NODE_NAME:-"Bootstrap Energy Validator"}
RPC_ARGS=(--rpc-cors all --rpc-methods unsafe)

pushd "${REPO_ROOT}" >/dev/null

if [[ ! -x "${BINARY}" ]]; then
  echo "building node binaries..."
  cargo build --release --features worldos-testnet
fi

echo "starting bootstrap validator ${NODE_NAME}"
"${BINARY}" \
  --chain "${CHAIN_ID}" \
  --validator \
  --name "${NODE_NAME}" \
  "${RPC_ARGS[@]}" &
NODE_PID=$!

echo "starting mock energy oracle"
(
  cd services/mock-energy-oracle
  cargo run --release
) &
ORACLE_PID=$!

if [[ -f docker/telemetry-stack.yml ]]; then
  echo "launching telemetry stack"
  docker-compose -f docker/telemetry-stack.yml up -d
fi

echo "world os testnet services launched"
echo "node pid: ${NODE_PID}"
echo "oracle pid: ${ORACLE_PID}"
wait
