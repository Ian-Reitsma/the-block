#!/usr/bin/env bash
set -euo pipefail

base="${1:-}"
head="${2:-}"

if [[ -z "$base" || -z "$head" ]]; then
  echo "usage: scripts/check_cla_range.sh <base> <head>" >&2
  exit 2
fi

commits=$(git rev-list "${base}..${head}")
if [[ -z "$commits" ]]; then
  echo "no commits to check between ${base} and ${head}"
  exit 0
fi

for commit in $commits; do
  ./scripts/check_cla.sh "$commit"
done
