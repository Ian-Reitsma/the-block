#!/usr/bin/env bash
# Wipe the legacy credits database from a node datadir.
# Usage: zero_credits_db.sh [path_to_datadir]
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
  echo "no credits.db at $DB" >&2
fi
