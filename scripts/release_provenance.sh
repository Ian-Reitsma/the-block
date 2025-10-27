#!/usr/bin/env bash
set -euo pipefail
TAG=${1:?"usage: $0 <tag>"}
OUTDIR="releases/$TAG"
mkdir -p "$OUTDIR"
SNAPSHOT_PATH="$OUTDIR/dependency-snapshot.json"
VENDOR_STAGE="$OUTDIR/vendor.staging"

# Ensure reproducible timestamps
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-0}

# Enforce dependency policy and capture the current registry snapshot.
cargo run -p dependency_registry -- --check --out-dir "$OUTDIR" \
  --snapshot "$SNAPSHOT_PATH" config/dependency_policies.toml

if [ ! -s "$SNAPSHOT_PATH" ]; then
  echo "dependency snapshot missing; ensure dependency_registry supports --snapshot" >&2
  exit 1
fi

CHECK_TELEMETRY_PATH="$OUTDIR/dependency-check.telemetry"
CHECK_SUMMARY_PATH="$OUTDIR/dependency-check.summary.json"
METRICS_TELEMETRY_PATH="$OUTDIR/dependency-metrics.telemetry"
REGISTRY_PATH="$OUTDIR/dependency-registry.json"
VIOLATIONS_PATH="$OUTDIR/dependency-violations.json"

for required in \
  "$CHECK_TELEMETRY_PATH" \
  "$CHECK_SUMMARY_PATH" \
  "$METRICS_TELEMETRY_PATH" \
  "$REGISTRY_PATH" \
  "$VIOLATIONS_PATH"
do
  if [ ! -s "$required" ]; then
    echo "release provenance missing artifact: $required" >&2
    exit 1
  fi
done

CHECK_TELEMETRY_HASH=$(sha256sum "$CHECK_TELEMETRY_PATH" | awk '{print $1}')
CHECK_SUMMARY_HASH=$(sha256sum "$CHECK_SUMMARY_PATH" | awk '{print $1}')
METRICS_TELEMETRY_HASH=$(sha256sum "$METRICS_TELEMETRY_PATH" | awk '{print $1}')
REGISTRY_HASH=$(sha256sum "$REGISTRY_PATH" | awk '{print $1}')
VIOLATIONS_HASH=$(sha256sum "$VIOLATIONS_PATH" | awk '{print $1}')

# Freeze the vendor tree for provenance and hash it deterministically.
rm -rf "$VENDOR_STAGE"
cargo vendor --locked --versioned-dirs "$VENDOR_STAGE" >/dev/null
if [ ! -d "$VENDOR_STAGE" ]; then
  echo "cargo vendor failed to populate $VENDOR_STAGE" >&2
  exit 1
fi
VENDOR_HASH=$(cd "$VENDOR_STAGE" && tar --sort=name --owner=0 --group=0 --numeric-owner \
  --mtime=@"${SOURCE_DATE_EPOCH}" -cf - . | sha256sum | awk '{print $1}')
rm -rf "$VENDOR_STAGE"
echo "$VENDOR_HASH" > "$OUTDIR/vendor-sha256.txt"
SNAPSHOT_HASH=$(sha256sum "$SNAPSHOT_PATH" | awk '{print $1}')

# Run the chaos gating harness unless explicitly skipped.  This exercises the
# provider failover drills and blocks releases when overlay readiness regresses
# or simulated outages fail to emit `/chaos/status` diffs.  Set
# `SKIP_CHAOS_GATING=1` to bypass (useful for reproducing failures locally).
if [[ ${SKIP_CHAOS_GATING:-0} != 1 ]]; then
  CHAOS_OUTDIR="$OUTDIR/chaos"
  mkdir -p "$CHAOS_OUTDIR"
  CHAOS_ARGS=(xtask chaos --out-dir "$CHAOS_OUTDIR")
  if [[ -n ${TB_CHAOS_STATUS_ENDPOINT:-} ]]; then
    CHAOS_ARGS+=(--status-endpoint "$TB_CHAOS_STATUS_ENDPOINT")
  fi
  if [[ -n ${TB_CHAOS_STEPS:-} ]]; then
    CHAOS_ARGS+=(--steps "$TB_CHAOS_STEPS")
  fi
  if [[ -n ${TB_CHAOS_NODE_COUNT:-} ]]; then
    CHAOS_ARGS+=(--nodes "$TB_CHAOS_NODE_COUNT")
  fi
  if [[ ${TB_CHAOS_REQUIRE_DIFF:-0} != 0 ]]; then
    CHAOS_ARGS+=(--require-diff)
  fi
  echo "Running chaos verification: cargo ${CHAOS_ARGS[*]}"
  cargo "${CHAOS_ARGS[@]}"
  for artifact in \
    "$CHAOS_OUTDIR/status.snapshot.json" \
    "$CHAOS_OUTDIR/status.diff.json" \
    "$CHAOS_OUTDIR/overlay.readiness.json" \
    "$CHAOS_OUTDIR/provider.failover.json"
  do
    if [[ ! -s "$artifact" ]]; then
      echo "chaos gating missing artifact: $artifact" >&2
      exit 1
    fi
  done
else
  echo "SKIP_CHAOS_GATING=1 — skipping chaos verification" >&2
fi

# Build binaries
cargo build --release

# Generate SBOM for binary
if command -v cargo-bom >/dev/null 2>&1; then
  cargo bom --format cyclonedx > "$OUTDIR/SBOM-x86_64.json"
elif command -v cargo auditable >/dev/null 2>&1; then
  cargo auditable build --release
  cp target/release/the_block "$OUTDIR/"
  cargo auditable sbom -o "$OUTDIR/SBOM-x86_64.json"
else
  echo "cargo-bom or cargo auditable required" >&2
  exit 1
fi

# Checksums
(
  cd "$OUTDIR"
  : > checksums.txt
  while IFS= read -r entry; do
    sha256sum "$entry" >> checksums.txt
  done < <(find . -maxdepth 1 -type f -printf '%P\n' | sort)
)
printf "vendor-tree  %s\n" "$VENDOR_HASH" >> "$OUTDIR/checksums.txt"

# Collect toolchain metadata
RUSTC_VER=$(rustc -V)
RUSTC_HASH=$(rustc -Vv | sed -n 's/commit-hash: //p')
LINKER_VER=$(ld --version 2>/dev/null | head -n1 || lld --version 2>/dev/null | head -n1)
cat > "$OUTDIR/provenance.json" <<JSON
{
  "tag": "$TAG",
  "toolchain": "$RUSTC_VER",
  "rustc_commit": "$RUSTC_HASH",
  "linker": "$LINKER_VER",
  "commit": "$(git rev-parse HEAD)",
  "repo": "$(git config --get remote.origin.url)",
  "source_date_epoch": "${SOURCE_DATE_EPOCH}",
  "dependency_snapshot": {
    "path": "$(basename "$SNAPSHOT_PATH")",
    "sha256": "$SNAPSHOT_HASH"
  },
  "dependency_check": {
    "registry": {
      "path": "$(basename "$REGISTRY_PATH")",
      "sha256": "$REGISTRY_HASH"
    },
    "violations": {
      "path": "$(basename "$VIOLATIONS_PATH")",
      "sha256": "$VIOLATIONS_HASH"
    },
    "telemetry": {
      "path": "$(basename "$CHECK_TELEMETRY_PATH")",
      "sha256": "$CHECK_TELEMETRY_HASH"
    },
    "summary": {
      "path": "$(basename "$CHECK_SUMMARY_PATH")",
      "sha256": "$CHECK_SUMMARY_HASH"
    }
  },
  "dependency_metrics": {
    "path": "$(basename "$METRICS_TELEMETRY_PATH")",
    "sha256": "$METRICS_TELEMETRY_HASH"
  },
  "vendor_tree_sha256": "$VENDOR_HASH"
}
JSON

# Sign artifacts
if command -v cosign >/dev/null 2>&1; then
  cosign attest --predicate "$OUTDIR/provenance.json" --type slsa-provenance "$OUTDIR/checksums.txt"
else
  echo "cosign not installed; skipping signatures" >&2
fi

echo "Release artifacts written to $OUTDIR"
