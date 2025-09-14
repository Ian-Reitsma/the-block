#!/usr/bin/env bash
set -euo pipefail

{
  echo '# Node Dependency Tree'
  echo
  echo 'This document lists the dependency hierarchy for the `the_block` node crate. It is generated via `cargo tree --manifest-path node/Cargo.toml`.'
  echo
  echo '```'
  cargo tree --manifest-path node/Cargo.toml
  echo '```'
} > docs/architecture/node.md

if command -v cargo-deps >/dev/null 2>&1 && command -v dot >/dev/null 2>&1; then
  cargo deps --manifest-path node/Cargo.toml --depth 1 | dot -Tsvg -o docs/architecture/node-deps.svg
fi
