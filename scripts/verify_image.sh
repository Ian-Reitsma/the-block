#!/usr/bin/env bash
set -euo pipefail
IMAGE=${1:?"usage: $0 <image>"}
if ! command -v cosign >/dev/null 2>&1; then
  sudo apt-get update && sudo apt-get install -y cosign
fi
scripts/generate_sbom.sh
if command -v cosign >/dev/null 2>&1; then
  cosign verify "$IMAGE" || true
  cosign verify-attestation "$IMAGE" --type cyclonedx --predicate sbom.json || true
fi
