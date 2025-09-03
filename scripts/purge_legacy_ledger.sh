#!/usr/bin/env bash
# Wipe the legacy ledger database from a node datadir.
# Usage: purge_legacy_ledger.sh [path_to_datadir]
set -euo pipefail

DIR="${1:-$HOME/.the_block}"
DB="$DIR/credits.db"

echo "checking $DB" >&2

if [ ! -d "$DIR" ]; then
  echo "datadir $DIR not found" >&2
  exit 1
fi

if [ -f "$DB" ]; then
  rm -v "$DB" >&2
  echo "removed legacy ledger" >&2
else
  echo "no legacy ledger at $DB" >&2
fi
