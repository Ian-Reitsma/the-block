#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

OUTPUT_DIR=${TB_DEPENDENCY_FAULT_OUT:-sim/output/dependency_fault}
LABEL=${TB_DEPENDENCY_FAULT_LABEL:-ci}

args=()
if [[ $# -eq 0 ]]; then
  args+=("--runtime" "tokio")
  args+=("--transport" "quinn")
  args+=("--overlay" "libp2p")
  args+=("--storage" "rocksdb")
  args+=("--coding" "reed-solomon")
  args+=("--crypto" "dalek")
  args+=("--codec" "bincode")
fi

args+=("--output-dir" "$OUTPUT_DIR")
args+=("--label" "$LABEL")

cargo run -p tb-sim --bin dependency_fault --features dependency-fault "${args[@]}" "$@"
