#!/usr/bin/env bash
set -euo pipefail
SRC_DIR=${1:-fuzz/wal}
DST_DIR=${2:-fuzz/corpus/wal}
mkdir -p "$DST_DIR"
while read -r file seed; do
  src="$SRC_DIR/$file"
  dst="$DST_DIR/seed-$seed"
  if [ -f "$src" ] && [ ! -f "$dst" ]; then
    cp "$src" "$dst"
  fi
done < <(scripts/extract_wal_seeds.sh "$SRC_DIR")
find "$DST_DIR" -type f -mtime +30 -delete
