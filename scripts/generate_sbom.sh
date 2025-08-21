#!/usr/bin/env bash
set -euo pipefail
output=${1:-sbom.json}
# Generate a minimal SBOM using cargo metadata
cargo metadata --format-version 1 > "$output"
echo "SBOM written to $output"
