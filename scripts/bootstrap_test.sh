#!/usr/bin/env bash
set -euo pipefail
tmp=$(mktemp -d)
cp bootstrap.sh "$tmp/bootstrap.sh"
# create minimal npm files so bootstrap doesn't warn about missing package.json
cat <<'EOF' > "$tmp/package.json"
{}
EOF
cat <<'EOF' > "$tmp/package-lock.json"
{}
EOF
chmod +x "$tmp/bootstrap.sh"
cd "$tmp"
./bootstrap.sh >/tmp/bootstrap_test.log
export PATH="$tmp/.venv/bin:$PATH"
if [[ "$(which python)" != "$tmp/.venv/bin/python" ]]; then
  echo "python shim not on PATH" >&2
  exit 1
fi
echo "bootstrap exposed project python"
CARGO_MAKE_VERSION="0.37.24"
if ! cargo make --version 2>/dev/null | grep -q "$CARGO_MAKE_VERSION"; then
  echo "cargo-make $CARGO_MAKE_VERSION missing" >&2
  exit 1
fi
# Pin to the last cargo-nextest release supporting rustc 1.82
NEXTEST_VERSION="0.9.97-b.2"
if ! cargo nextest --version 2>/dev/null | grep -q "$NEXTEST_VERSION"; then
  echo "cargo-nextest $NEXTEST_VERSION missing" >&2
  exit 1
fi
