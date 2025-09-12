#!/usr/bin/env bash
set -euo pipefail
file="$1"
if ! command -v jq >/dev/null; then
  echo "jq required" >&2
  exit 1
fi
jq '.region and (.consent_required|type=="boolean") and (.features|type=="array")' "$file" >/dev/null
