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
NEXTEST_VERSION="0.9.102"
if cargo nextest --version 2>/dev/null | grep -q "$NEXTEST_VERSION"; then
  echo "cargo-nextest $NEXTEST_VERSION already installed"
else
  cargo +1.87.0 install cargo-nextest --force >/tmp/nextest_install.log && tail -n 20 /tmp/nextest_install.log
fi
