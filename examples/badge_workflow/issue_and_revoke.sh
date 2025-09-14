#!/usr/bin/env bash
set -euo pipefail
BADGE=$(cargo run --quiet --bin contract -- service-badge issue | jq -r '.badge')
printf "issued: %s\n" "$BADGE"
cargo run --quiet --bin contract -- service-badge verify --badge "$BADGE"
cargo run --quiet --bin contract -- service-badge revoke
