#!/usr/bin/env bash
set -euo pipefail

# Developer bootstrap: extends bootstrap.sh with additional tooling.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$SCRIPT_DIR/bootstrap.sh"

# Ensure cargo-nextest and npm deps are installed
cargo install cargo-nextest --locked >/dev/null 2>&1 || true
npm ci --prefix monitoring >/dev/null 2>&1 || true

# Launch a small local cluster
"$SCRIPT_DIR/swarm.sh" up
