#!/usr/bin/env bash
set -euo pipefail
DIR=${1:-fuzz/wal}
for f in "$DIR"/*; do
  [ -f "$f" ] || continue
  seed=$(strings "$f" 2>/dev/null | grep -o 'seed: [0-9]*' | head -n1 | awk '{print $2}' || true)
  if [ -n "${seed:-}" ]; then
    echo "$(basename "$f") $seed"
  fi
done
