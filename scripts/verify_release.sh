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
snapshot_path="$(dirname "$ARCHIVE")/dependency-snapshot.json"
if [ -f "$snapshot_path" ]; then
  echo "Dependency snapshot at $snapshot_path"
  baseline_path=${TB_DEPENDENCY_BASELINE:-docs/dependency_inventory.json}
  if [ -f "$baseline_path" ]; then
    python3 - "$baseline_path" "$snapshot_path" <<'PY'
import json
import sys
from pathlib import Path

baseline_path = Path(sys.argv[1])
snapshot_path = Path(sys.argv[2])

def canonicalise(registry):
    if "generated_at" in registry:
        registry = {
            "root_packages": registry.get("root_packages", []),
            "policy": registry.get("policy", {}),
            "entries": registry.get("entries", []),
        }

    def sort_refs(refs):
        return sorted(refs, key=lambda item: (item.get("name"), item.get("version")))

    entries = []
    for entry in registry.get("entries", []):
        normalised = dict(entry)
        normalised["dependencies"] = sort_refs(normalised.get("dependencies", []))
        normalised["dependents"] = sort_refs(normalised.get("dependents", []))
        entries.append(normalised)

    entries.sort(key=lambda item: (item.get("name"), item.get("version")))

    policy = dict(registry.get("policy", {}))
    if "forbidden_licenses" in policy:
        policy["forbidden_licenses"] = sorted(policy["forbidden_licenses"])

    return {
        "root_packages": sorted(registry.get("root_packages", [])),
        "policy": policy,
        "entries": entries,
    }

with baseline_path.open("r", encoding="utf-8") as handle:
    baseline = json.load(handle)
with snapshot_path.open("r", encoding="utf-8") as handle:
    snapshot = json.load(handle)

if canonicalise(baseline) != canonicalise(snapshot):
    print(
        f"warning: dependency snapshot differs from baseline {baseline_path}",
        file=sys.stderr,
    )
else:
    print("Dependency snapshot matches baseline")
PY
  else
    echo "warning: dependency baseline missing at $baseline_path" >&2
  fi
else
  echo "warning: dependency snapshot missing" >&2
fi
vendor_hash=$(awk '$1 == "vendor-tree" {print $2}' "$CHECKS" || true)
if [ -n "$vendor_hash" ]; then
  echo "Vendor tree sha256: $vendor_hash"
else
  echo "warning: vendor hash missing from $CHECKS" >&2
fi
