#!/usr/bin/env bash
set -euo pipefail
if [ $# -lt 1 ]; then
  echo "usage: $0 <crash-file>" >&2
  exit 1
fi
cargo fuzz run network -- "$1"
