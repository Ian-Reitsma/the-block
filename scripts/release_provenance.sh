#!/usr/bin/env bash
set -euo pipefail
TAG=${1:?"usage: $0 <tag>"}
OUTDIR="releases/$TAG"
mkdir -p "$OUTDIR"

# Ensure reproducible timestamps
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-0}

# Enforce dependency policy and capture the current registry snapshot.
cargo run -p dependency_registry -- --check --out-dir "$OUTDIR" config/dependency_policies.toml

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
  "source_date_epoch": "${SOURCE_DATE_EPOCH}" 
}
JSON

# Sign artifacts
if command -v cosign >/dev/null 2>&1; then
  cosign attest --predicate "$OUTDIR/provenance.json" --type slsa-provenance "$OUTDIR/checksums.txt"
else
  echo "cosign not installed; skipping signatures" >&2
fi

echo "Release artifacts written to $OUTDIR"
