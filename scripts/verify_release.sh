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

chaos_dir="$(dirname "$ARCHIVE")/chaos"
if [ -d "$chaos_dir" ]; then
  echo "Chaos verification artifacts present:"
  missing=0
  for artifact in \
    "status.snapshot.json" \
    "status.diff.json" \
    "overlay.readiness.json" \
    "provider.failover.json"
  do
    path="$chaos_dir/$artifact"
    if [ -s "$path" ]; then
      echo "  $(basename "$path")"
    else
      echo "  missing or empty: $artifact" >&2
      missing=1
    fi
  done
  if [ "$missing" -ne 0 ]; then
    exit 1
  fi
  archive_dir="$chaos_dir/archive"
  latest_manifest="$archive_dir/latest.json"
  if [ ! -s "$latest_manifest" ]; then
    echo "warning: chaos archive latest manifest missing at $latest_manifest" >&2
    exit 1
  fi
  python3 - "$archive_dir" "$latest_manifest" <<'PY'
import json
import sys
from pathlib import Path

archive_root = Path(sys.argv[1])
latest_manifest = Path(sys.argv[2])

with latest_manifest.open("r", encoding="utf-8") as handle:
    latest = json.load(handle)

manifest_rel = latest.get("manifest")
if not manifest_rel:
    print(f"warning: chaos archive latest manifest missing 'manifest' field", file=sys.stderr)
    sys.exit(1)

manifest_path = archive_root / manifest_rel
if not manifest_path.exists():
    print(f"warning: chaos archive manifest missing at {manifest_path}", file=sys.stderr)
    sys.exit(1)

with manifest_path.open("r", encoding="utf-8") as handle:
    manifest = json.load(handle)

run_id = manifest.get("run_id")
artifacts = manifest.get("artifacts", {})
if not artifacts:
    print("warning: chaos archive manifest contains no artefacts", file=sys.stderr)
    sys.exit(1)

bundle = manifest.get("bundle")
if not bundle:
    print("warning: chaos archive manifest missing bundle metadata", file=sys.stderr)
    sys.exit(1)

bundle_file = bundle.get("file")
if not bundle_file:
    print("warning: chaos archive bundle missing 'file' entry", file=sys.stderr)
    sys.exit(1)

bundle_path = archive_root / bundle_file
if not bundle_path.exists() or bundle_path.stat().st_size == 0:
    print(f"warning: archived chaos bundle missing at {bundle_path}", file=sys.stderr)
    sys.exit(1)

recorded_size = bundle.get("size")
if recorded_size is not None and bundle_path.stat().st_size != recorded_size:
    print(
        f"warning: bundle size mismatch for {bundle_path} (expected {recorded_size}, found {bundle_path.stat().st_size})",
        file=sys.stderr,
    )
    sys.exit(1)

missing = []
for name, meta in artifacts.items():
    filename = meta.get("file")
    if not filename:
        missing.append((name, "<unknown>"))
        continue
    artefact_path = archive_root / run_id / filename if run_id else archive_root / filename
    if not artefact_path.exists() or artefact_path.stat().st_size == 0:
        missing.append((name, str(artefact_path)))

if missing:
    for name, path in missing:
        print(f"warning: archived chaos artefact '{name}' missing at {path}", file=sys.stderr)
    sys.exit(1)
PY
  diff_path="$chaos_dir/status.diff.json"
  python3 - "$diff_path" <<'PY'
import json
import sys
from pathlib import Path

diff_path = Path(sys.argv[1])
if not diff_path.exists():
    print(f"warning: chaos diff missing at {diff_path}", file=sys.stderr)
    sys.exit(1)

try:
    with diff_path.open("r", encoding="utf-8") as handle:
        diff = json.load(handle)
except json.JSONDecodeError as exc:
    print(f"warning: failed to parse chaos diff: {exc}", file=sys.stderr)
    sys.exit(1)

overlay_failures = []
epsilon = 1e-6
has_overlay_entries = False
for entry in diff:
    if entry.get("module") != "overlay":
        continue
    has_overlay_entries = True
    before = entry.get("readiness_before")
    after = entry.get("readiness_after")
    if isinstance(before, (int, float)) and isinstance(after, (int, float)):
        if after + epsilon < before:
            overlay_failures.append(
                f"scenario {entry.get('scenario')} readiness dropped {before:.3f} -> {after:.3f}"
            )
    removed = entry.get("site_removed") or []
    if removed:
        sites = ", ".join(str(item.get("site")) for item in removed)
        overlay_failures.append(
            f"scenario {entry.get('scenario')} lost provider sites: {sites}"
        )

if not has_overlay_entries:
    overlay_failures.append("chaos/status diff did not include overlay module entries")

if overlay_failures:
    for failure in overlay_failures:
        print(f"warning: overlay regression detected: {failure}", file=sys.stderr)
    sys.exit(1)
PY
else
  echo "warning: chaos verification artifacts missing (expected directory $chaos_dir)" >&2
  exit 1
fi
