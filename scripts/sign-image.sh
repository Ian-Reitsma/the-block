#!/usr/bin/env bash
# Sign container image using cosign.
set -euo pipefail
IMG=$1
cosign sign --key cosign.key "$IMG"
