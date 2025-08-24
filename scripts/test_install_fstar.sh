
#!/usr/bin/env bash
set -euo pipefail
tmp=$(mktemp -d)
cp scripts/install_fstar.sh "$tmp"/install_fstar.sh
pushd "$tmp" >/dev/null
unset FSTAR_HOME
if FSTAR_VERSION=bogus ./install_fstar.sh >out.log 2>&1; then
  echo "install_fstar unexpectedly succeeded" >&2
  exit 1
fi
if ! grep -q 'F★ release bogus not found' out.log; then
  cat out.log >&2
  echo "error message missing" >&2
  exit 1
fi
echo "installer failed as expected"
