#!/usr/bin/env bash
set -euo pipefail
if [[ ${1:-} == "-h" || ${1:-} == "--help" ]]; then
  echo "usage: $0 <archive> <checksums.txt> <signature>"
  exit 0
fi

ARCHIVE=${1:?"usage: $0 <archive> <checksums.txt> <signature>"}
CHECKS=${2:?"usage: $0 <archive> <checksums.txt> <signature>"}
SIG=${3:?"usage: $0 <archive> <checksums.txt> <signature>"}

sha=$(sha256sum "$ARCHIVE" | awk '{print $1}')
grep "$sha  $(basename "$ARCHIVE")" "$CHECKS" >/dev/null
if command -v cosign >/dev/null 2>&1; then
  cosign verify-blob --signature "$SIG" --digest "sha256:$sha" "$CHECKS"
else
  echo "cosign not installed; signature verification skipped" >&2
fi
sbom=$(ls "$(dirname "$ARCHIVE")"/SBOM-*.json 2>/dev/null | head -n1 || true)
echo "SBOM at ${sbom:-<missing>}"
snapshot_path="$(dirname "$ARCHIVE")/dependency-registry.json"
if [ -f "$snapshot_path" ]; then
  echo "Dependency snapshot at $snapshot_path"
else
  echo "warning: dependency snapshot missing" >&2
fi
