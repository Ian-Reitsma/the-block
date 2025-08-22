#!/usr/bin/env bash
set -euo pipefail
if FSTAR_VERSION=bogus scripts/install_fstar.sh >/tmp/fstar.log 2>&1; then
  echo "install_fstar unexpectedly succeeded" >&2
  exit 1
else
  echo "installer failed as expected"
  exit 0
fi
