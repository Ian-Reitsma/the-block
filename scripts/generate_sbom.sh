#!/usr/bin/env bash
set -euo pipefail
output=${1:-sbom.json}
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-0}
if command -v cargo-bom >/dev/null 2>&1; then
  cargo bom --format cyclonedx > "$output"
elif command -v cargo auditable >/dev/null 2>&1; then
  cargo auditable sbom -o "$output"
else
  cargo metadata --format-version 1 > "$output"
fi
echo "SBOM written to $output"
