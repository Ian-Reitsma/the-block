#!/usr/bin/env bash
set -euo pipefail

cargo deps --all-deps --manifest-path node/Cargo.toml -o docs/architecture/node.dot
if command -v dot >/dev/null 2>&1; then
  dot -Tsvg docs/architecture/node.dot -o docs/architecture/node.svg
fi
