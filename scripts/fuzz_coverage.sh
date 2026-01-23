#!/usr/bin/env bash
set -euo pipefail

have_llvm_tools() {
  command -v llvm-profdata >/dev/null && command -v llvm-cov >/dev/null
}

if ! have_llvm_tools; then
  if command -v rustup >/dev/null; then
    echo "llvm tools missing; installing via rustup" >&2
    if rustup component add llvm-tools-preview >/tmp/llvm-tools.log 2>&1; then
      host_triple=$(rustc -Vv | grep host | awk '{print $2}')
      export PATH="$(rustc --print sysroot)/lib/rustlib/${host_triple}/bin:$PATH"
    else
      cat /tmp/llvm-tools.log >&2
    fi
  elif command -v apt-get >/dev/null; then
    echo "llvm tools missing; installing via apt" >&2
    if apt-get update >/tmp/llvm-tools.log 2>&1 && \
       apt-get install -y llvm llvm-profdata llvm-cov >/tmp/llvm-tools.log 2>&1; then
      :
    else
      cat /tmp/llvm-tools.log >&2
    fi
  elif command -v brew >/dev/null; then
    echo "llvm tools missing; installing via brew" >&2
    if brew install llvm >/tmp/llvm-tools.log 2>&1; then
      export PATH="$(brew --prefix llvm)/bin:$PATH"
    else
      cat /tmp/llvm-tools.log >&2
    fi
  elif command -v pacman >/dev/null; then
    echo "llvm tools missing; installing via pacman" >&2
    if pacman -Sy --noconfirm llvm >/tmp/llvm-tools.log 2>&1; then
      :
    else
      cat /tmp/llvm-tools.log >&2
    fi
  fi
fi

if ! have_llvm_tools; then
  echo "llvm tools required (install llvm-profdata and llvm-cov)" >&2
  exit 1
fi

out_dir=${1:-fuzz/coverage}
mkdir -p "$out_dir"
profraws=$(find fuzz net/fuzz gateway/fuzz -maxdepth 1 -name '*.profraw' -print)
if [ -z "$profraws" ]; then
  echo "no profraw files; run fuzz targets with coverage instrumentation first" >&2
  exit 0
fi
llvm-profdata merge -sparse $profraws -o fuzz/coverage.profdata

bins=()
for p in $profraws; do
  dir=$(dirname "$p")
  # Include the new energy_receipt fuzz target (produces energy_receipt-*.profraw).
  name=$(basename "$p" | cut -d- -f1)
  bin=""
  if [ -d "$dir/target" ]; then
    bin=$(find "$dir/target" -path "*/release/$name" -type f -perm -u+x | head -n 1)
  fi
  if [ -z "$bin" ]; then
    bin=$(find target -path "*/release/$name" -type f -perm -u+x 2>/dev/null | head -n 1)
  fi
  [ -n "$bin" ] && bins+=("$bin")
done
[ ${#bins[@]} -eq 0 ] && { echo "no fuzz binaries" >&2; exit 1; }
llvm-cov show "${bins[@]}" \
  -instr-profile=fuzz/coverage.profdata \
  -format=html -output-dir="$out_dir" >/dev/null
echo "coverage report generated at $out_dir" >&2
