#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<USAGE
Usage: scripts/fuzz_coverage.sh [OUT_DIR] [options]

Generates coverage HTML for fuzz binaries by optionally running fuzz targets with coverage instrumentation,
merging the resulting .profraw files, and invoking llvm-cov.

Options:
  OUT_DIR            Directory for coverage artifacts (default: fuzz/coverage).
  --target <name>    Run the named fuzz target before merging (.profraw files go to <OUT_DIR>/profraw).
  --targets <list>   Comma-separated list of targets.
  --duration <secs>  Seconds to limit each fuzz run (defaults to 30). Use 0 to run until manual stop.
  --no-run           Skip running fuzz targets and only merge existing .profraw files.
  --help             Show this message.
USAGE
}

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

out_dir="fuzz/coverage"
if [ $# -gt 0 ] && [[ "$1" != --* ]]; then
  out_dir="$1"
  shift
fi

skip_run=0
duration=30
run_targets=()

while [ $# -gt 0 ]; do
  case "$1" in
    --target)
      shift
      [ $# -gt 0 ] || { echo "--target requires an argument" >&2; exit 1; }
      run_targets+=("$1")
      shift
      ;;
    --target=*)
      run_targets+=("${1#*=}")
      shift
      ;;
    --targets)
      shift
      [ $# -gt 0 ] || { echo "--targets requires an argument" >&2; exit 1; }
      IFS=, read -ra more_targets <<< "$1"
      for target in "${more_targets[@]}"; do
        run_targets+=("$target")
      done
      shift
      ;;
    --targets=*)
      IFS=, read -ra more_targets <<< "${1#*=}"
      for target in "${more_targets[@]}"; do
        run_targets+=("$target")
      done
      shift
      ;;
    --duration)
      shift
      [ $# -gt 0 ] || { echo "--duration requires an argument" >&2; exit 1; }
      duration="$1"
      shift
      ;;
    --duration=*)
      duration="${1#*=}"
      shift
      ;;
    --no-run|--skip-run)
      skip_run=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if ! [[ "$duration" =~ ^[0-9]+$ ]]; then
  echo "--duration must be a non-negative integer" >&2
  exit 1
fi

mkdir -p "$out_dir"
profraw_dir="$out_dir/profraw"
mkdir -p "$profraw_dir"

if [ "$skip_run" -eq 0 ]; then
  if [[ "${RUSTFLAGS:-}" != *"-C instrument-coverage"* ]]; then
    export RUSTFLAGS="${RUSTFLAGS:-} -C instrument-coverage"
  fi
  if [ ${#run_targets[@]} -eq 0 ]; then
    run_targets=("compute_market")
  fi
  for target in "${run_targets[@]}"; do
    echo "Fuzz coverage: running target '$target' for ${duration:-0} seconds" >&2
    export LLVM_PROFILE_FILE="$profraw_dir/${target}-%p.profraw"
    cmd=(cargo fuzz run "$target")
    if [ "$duration" -gt 0 ]; then
      cmd+=(-- "-max_total_time=$duration")
    fi
    "${cmd[@]}"
  done
fi

profraws=()
for dir in fuzz net/fuzz gateway/fuzz "$profraw_dir"; do
  if [ -d "$dir" ]; then
    while IFS= read -r file; do
      profraws+=("$file")
    done < <(find "$dir" -maxdepth 1 -name '*.profraw' -print)
  fi
done

if [ ${#profraws[@]} -eq 0 ]; then
  echo "no profraw files; run fuzz targets with coverage instrumentation first" >&2
  exit 0
fi

coverage_profile="$out_dir/coverage.profdata"
llvm-profdata merge -sparse "${profraws[@]}" -o "$coverage_profile"

bins=()
for p in "${profraws[@]}"; do
  name=$(basename "$p" | cut -d- -f1)
  bin=$(find fuzz/target target -path "*/release/$name" -type f -perm -u+x 2>/dev/null | head -n 1)
  [ -n "$bin" ] && bins+=("$bin")
done

[ ${#bins[@]} -eq 0 ] && { echo "no fuzz binaries" >&2; exit 1; }

llvm-cov show "${bins[@]}" \
  -instr-profile="$coverage_profile" \
  -format=html -output-dir="$out_dir" >/dev/null
echo "coverage report generated at $out_dir" >&2
