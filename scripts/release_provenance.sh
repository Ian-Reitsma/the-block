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
if [ ! -s "$CHECK_TELEMETRY_PATH" ]; then
  echo "dependency check telemetry missing; ensure dependency_registry emits dependency-check.telemetry" >&2
  exit 1
fi

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
CHECK_TELEMETRY_HASH=$(sha256sum "$CHECK_TELEMETRY_PATH" | awk '{print $1}')

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
( cd "$OUTDIR" && sha256sum * > checksums.txt )
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
  "dependency_check_telemetry": {
    "path": "$(basename "$CHECK_TELEMETRY_PATH")",
    "sha256": "$CHECK_TELEMETRY_HASH"
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
