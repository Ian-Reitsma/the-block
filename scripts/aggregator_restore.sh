#!/bin/sh
set -e
BACKUP=${1:?"backup path required"}
DB=${2:-peer_metrics.db}
rm -rf "$DB"
cp -r "$BACKUP" "$DB"
echo "restored $BACKUP to $DB"
