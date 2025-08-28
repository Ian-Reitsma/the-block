#!/usr/bin/env bash
set -euo pipefail

DB_PATH="${1:-chain_db}"

# Compact and verify sled database at DB_PATH
cargo run --quiet --bin db_compact "$DB_PATH"
