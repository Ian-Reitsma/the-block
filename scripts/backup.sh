#!/usr/bin/env bash
set -euo pipefail
DB_DIR=${1:-state}
OUT=${2:-backups}
ts=$(date +%Y%m%d%H%M%S)
mkdir -p "$OUT"
tar czf "$OUT/snapshot-$ts.tar.gz" "$DB_DIR"
if [ -d "$DB_DIR/wal" ]; then
  tar czf "$OUT/wal-$ts.tar.gz" "$DB_DIR/wal"
fi
echo "backup written to $OUT"
