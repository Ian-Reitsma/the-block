#!/usr/bin/env bash
set -euo pipefail

DETACH="${DETACH:-0}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MON_DIR="$ROOT_DIR/monitoring"
OUTPUT_DIR="$MON_DIR/output"
SCRIPT="$MON_DIR/tools/render_foundation_dashboard.py"
ENDPOINT="${TELEMETRY_ENDPOINT:-http://localhost:9898/metrics}"
REFRESH_INTERVAL="${REFRESH_INTERVAL:-5}"
PORT="${FOUNDATION_DASHBOARD_PORT:-8088}"

if [[ ! -x "$SCRIPT" ]]; then
  echo "missing dashboard generator: $SCRIPT" >&2
  exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

render_loop() {
  while true; do
    python3 "$SCRIPT" "$ENDPOINT" || true
    sleep "$REFRESH_INTERVAL"
  done
}

render_loop &
RENDER_PID=$!

cleanup() {
  kill "$RENDER_PID" "$SERVER_PID" 2>/dev/null || true
  if [[ "${DETACH}" -eq 0 ]]; then
    rm -rf "$OUTPUT_DIR"
  fi
}
trap cleanup EXIT

python3 "$SCRIPT" "$ENDPOINT"

cd "$OUTPUT_DIR"
if [[ "$DETACH" -eq 1 ]]; then
  python3 -m http.server "$PORT" --bind 127.0.0.1 >/dev/null 2>&1 &
  SERVER_PID=$!
  echo "Foundation telemetry dashboard available on http://127.0.0.1:${PORT}"
  echo "Renderer PID: $RENDER_PID, server PID: $SERVER_PID"
  exit 0
fi

python3 -m http.server "$PORT" --bind 127.0.0.1 &
SERVER_PID=$!
echo "Foundation telemetry dashboard serving on http://127.0.0.1:${PORT}"
wait "$SERVER_PID"
