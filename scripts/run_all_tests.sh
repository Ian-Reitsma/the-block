#!/usr/bin/env bash
# run_all_tests.sh — build the wheel and run all tests.
# Optional Cargo features are auto-detected via `cargo metadata | jq`.
# If `jq` is missing, the script warns and proceeds without optional features
# so minimal images can still execute the test suite.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"
if [[ -z "${VIRTUAL_ENV:-}" || "$(which python)" != "$REPO_ROOT/.venv/bin/python" ]]; then
  echo "Error: activate the venv at $REPO_ROOT/.venv before running." >&2
  exit 1
fi

FEATURE_CANDIDATES=(fuzzy telemetry)
SELECTED_FEATURES=()
if command -v jq >/dev/null 2>&1; then
  AVAILABLE_FEATURES=$(cargo metadata --no-deps --format-version=1 | jq -r '.packages[] | select(.name=="the_block") | .features | keys[]')
  for feat in "${FEATURE_CANDIDATES[@]}"; do
    if grep -qx "$feat" <<<"$AVAILABLE_FEATURES"; then
      SELECTED_FEATURES+=("$feat")
    else
      echo "Warning: skipping unsupported feature '$feat'" >&2
    fi
  done
else
  echo "Warning: jq not installed; skipping feature detection" >&2
fi
if [[ ${#SELECTED_FEATURES[@]} -gt 0 ]]; then
  FEATURE_FLAG="--features $(IFS=,; echo "${SELECTED_FEATURES[*]}")"
else
  FEATURE_FLAG=""
fi

maturin develop --release $FEATURE_FLAG
cargo test --all --release $FEATURE_FLAG
python -m pytest -q

# Run fuzz target when `cargo fuzz` is available. `cargo fuzz --help` returns
# non-zero if the subcommand is missing, so guard the call and emit a warning
# instead of failing hard.
if cargo fuzz --help >/dev/null 2>&1; then
  FUZZ_RUNS=${FUZZ_RUNS:-100000}
  cargo fuzz run verify_sig -- -runs="$FUZZ_RUNS"
else
  echo "Warning: cargo-fuzz not installed; skipping fuzz tests" >&2
fi

if [[ "${RUN_BENCH:-}" == "1" ]]; then
  cargo bench
fi
