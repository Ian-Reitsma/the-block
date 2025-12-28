#!/usr/bin/env bash
# Simple devnet faucet dispensing BLOCK (uses the legacy `--ct` flag).
# Usage: devnet_faucet.sh <to_address> [amount_nct]
set -euo pipefail
TO="$1"
AMOUNT="${2:-1000000}"
blockctl wallet transfer --ct "$AMOUNT" --to "$TO" --from faucet
