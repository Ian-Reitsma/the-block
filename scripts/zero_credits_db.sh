#!/usr/bin/env bash
# Wipe the legacy credits database from a node datadir.
# Usage: zero_credits_db.sh [path_to_datadir]
set -euo pipefail
DIR="${1:-$HOME/.the_block}"
DB="$DIR/credits.db"
if [ -f "$DB" ]; then
  rm -f "$DB"
  echo "removed $DB"
else
  echo "no credits.db at $DB"
fi
