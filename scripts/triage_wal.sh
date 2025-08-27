#!/usr/bin/env bash
set -euo pipefail
latest=$(ls -t fuzz/wal/* 2>/dev/null | head -n1 || true)
test -z "$latest" && { echo "No artifacts"; exit 0; }
echo "Replaying $latest"
RUST_BACKTRACE=1 cargo +nightly fuzz run wal_fuzz "$latest"
