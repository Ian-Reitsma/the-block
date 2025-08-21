#!/usr/bin/env bash
set -euo pipefail
commit=${1:-HEAD}
if ! git log -1 "$commit" --pretty=%B | grep -q "Signed-off-by:"; then
  echo "missing Signed-off-by" >&2
  exit 1
fi
