#!/bin/sh
set -e
DB=${1:-peer_metrics.db}
OUT=${2:-${DB}.bak}
cp -r "$DB" "$OUT"
echo "backup written to $OUT"
