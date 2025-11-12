#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="${PROFILE:-LOCAL_TWO_NODE}"
export PROFILE

cecho() {
  local color="$1"; shift
  if [[ -t 1 ]]; then
    case "$color" in
      red) tput setaf 1;;
      green) tput setaf 2;;
      yellow) tput setaf 3;;
      blue) tput setaf 4;;
      cyan) tput setaf 6;;
    esac
    echo -e "$*"
    tput sgr0
  else
    echo "$*"
  fi
}

cecho cyan "→ bootstrapping LOCAL_TWO_NODE profile"
bash "$ROOT/scripts/bootstrap.sh"

DATA_ROOT="${DATA_ROOT:-$ROOT/.localnet}"
NODE_A="$DATA_ROOT/node-a"
NODE_B="$DATA_ROOT/node-b"
LOG_DIR="$DATA_ROOT/logs"
mkdir -p "$NODE_A" "$NODE_B" "$LOG_DIR"

start_monitor() {
  if [[ "${DISABLE_LOCAL_DASHBOARD:-0}" == "1" ]]; then
    return
  fi
  if command -v docker >/dev/null 2>&1; then
    cecho cyan "→ starting dashboards (docker compose)"
    (cd "$ROOT/monitoring" && docker compose up) &
    MONITOR_PID=$!
  else
    cecho cyan "→ starting native monitor script"
    bash "$ROOT/scripts/monitor_native.sh" &
    MONITOR_PID=$!
  fi
}

start_node() {
  local name="$1"
  local data_dir="$2"
  local rpc_port="$3"
  local status_port="$4"
  local metrics_port="$5"
  local log_file="$LOG_DIR/${name}.log"
  cecho cyan "→ launching ${name} (rpc ${rpc_port}, status ${status_port})"
  RUST_LOG="${RUST_LOG:-info}" cargo run --bin node -- run \
    --rpc-addr "127.0.0.1:${rpc_port}" \
    --mempool-purge-interval 1 \
    --snapshot-interval 60 \
    --ack-privacy standard \
    --db-path "${data_dir}/db" \
    --data-dir "${data_dir}" \
    --log-format plain \
    --log-level info \
    --metrics-addr "127.0.0.1:${metrics_port}" \
    --status-addr "127.0.0.1:${status_port}" \
    >"${log_file}" 2>&1 &
  echo $!
}

trap 'kill 0 2>/dev/null || true' EXIT
start_monitor
NODE_A_PID=$(start_node "node-a" "$NODE_A" 9040 8140 9240)
NODE_B_PID=$(start_node "node-b" "$NODE_B" 9041 8141 9241)
cecho green "nodes running (PIDs: ${NODE_A_PID}, ${NODE_B_PID}); logs under ${LOG_DIR}"
wait -n
