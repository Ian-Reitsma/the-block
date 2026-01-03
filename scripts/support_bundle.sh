#!/usr/bin/env bash
set -euo pipefail
OUT="support-$(date -u +%Y%m%d-%H%M%S).tar.gz"
TMP=$(mktemp -d)
CONFIG=${CONFIG:-$HOME/.block/config.toml}
DATADIR=${DATADIR:-$HOME/.block/datadir}
LOG=${LOG:-$DATADIR/node.log}

redact() {
  sed -E 's/(admin_token\s*=\s*\").*(\")/\1REDACTED\2/; s/(private_key\s*=\s*\").*(\")/\1REDACTED\2/' "$1" > "$2"
}

if [ -f "$CONFIG" ]; then
  redact "$CONFIG" "$TMP/config.toml"
fi
if [ -f "$LOG" ]; then
  tail -c 10M "$LOG" > "$TMP/node.log"
fi
if curl -fsS http://localhost:9898/metrics > "$TMP/metrics.txt"; then
  true
fi
uname -a > "$TMP/uname.txt"

( cd "$TMP" && find . -type f -print0 | LC_ALL=C sort -z | tar --null -T - --no-recursion --owner=0 --group=0 --numeric-owner --exclude="$OUT" -czf "$OUT" )
mv "$TMP/$OUT" "$OUT"
rm -rf "$TMP"
echo "Bundle written to $OUT"
