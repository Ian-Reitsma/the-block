#!/usr/bin/env bash
set -euo pipefail

COUNT=${1:-1000000}
TMP_ROOT=${TMPDIR:-/tmp}/the-block-log-load
LOG_FILE="$TMP_ROOT/logs.json"
DB_FILE="$TMP_ROOT/logs.db"

mkdir -p "$TMP_ROOT"
rm -f "$LOG_FILE" "$DB_FILE"

echo "[log-indexer] generating ${COUNT} log lines at $LOG_FILE" >&2
python3 - "$LOG_FILE" "$COUNT" <<'PY'
import json
import random
import sys
from time import time

path = sys.argv[1]
count = int(sys.argv[2])
levels = ["INFO", "WARN", "ERROR", "DEBUG"]
now = int(time())
with open(path, "w", encoding="utf-8") as fh:
    for i in range(count):
        entry = {
            "timestamp": now + i,
            "level": random.choice(levels),
            "message": f"synthetic log {i}",
            "correlation_id": f"corr-{i % 97}",
        }
        fh.write(json.dumps(entry))
        fh.write("\n")
PY

echo "[log-indexer] indexing logs into $DB_FILE" >&2
cargo run --release --manifest-path tools/log_indexer_cli/Cargo.toml -- index "$LOG_FILE" "$DB_FILE"

echo "[log-indexer] verifying sample query" >&2
cargo run --manifest-path tools/log_indexer_cli/Cargo.toml -- search "$DB_FILE" --limit 5 --level ERROR >/dev/null

echo "[log-indexer] completed load test" >&2
