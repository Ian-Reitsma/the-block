#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"
if [[ -z "${VIRTUAL_ENV:-}" || "$(which python)" != "$REPO_ROOT/.venv/bin/python" ]]; then
  echo "Error: activate the venv at $REPO_ROOT/.venv before running." >&2
  exit 1
fi
maturin develop --release --features extension-module
cargo test --all --release
python -m pytest -q
if command -v cargo-fuzz >/dev/null; then
  FUZZ_RUNS=${FUZZ_RUNS:-100000}
  cargo fuzz run verify_sig -- -runs=$FUZZ_RUNS
fi
if [[ "${RUN_BENCH:-}" == "1" ]]; then
  cargo bench
fi
