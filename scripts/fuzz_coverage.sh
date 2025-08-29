#!/usr/bin/env bash
set -euo pipefail
if ! command -v llvm-profdata >/dev/null || ! command -v llvm-cov >/dev/null; then
  echo "llvm tools required" >&2
  exit 1
fi
out_dir=${1:-fuzz/coverage}
mkdir -p "$out_dir"
profraws=$(find fuzz -maxdepth 1 -name '*.profraw' -print)
[ -z "$profraws" ] && { echo "no profraw files" >&2; exit 1; }
llvm-profdata merge -sparse $profraws -o fuzz/coverage.profdata
llvm-cov show fuzz/target/x86_64-unknown-linux-gnu/release/compute_market \
  fuzz/target/x86_64-unknown-linux-gnu/release/network \
  -instr-profile=fuzz/coverage.profdata \
  -format=html -output-dir="$out_dir" >/dev/null
