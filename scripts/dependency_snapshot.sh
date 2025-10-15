#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT/target"

cargo run -p dependency_registry -- --out-dir "$OUT_DIR" --baseline "$ROOT/docs/dependency_inventory.json" "$@" >/dev/null

cp "$OUT_DIR/dependency-registry.json" "$ROOT/docs/dependency_inventory.json"
cp "$OUT_DIR/dependency-violations.json" "$ROOT/docs/dependency_inventory.violations.json"
cp "$OUT_DIR/dependency-check.telemetry" "$ROOT/docs/dependency_inventory.telemetry"

cat <<MSG
Dependency snapshot refreshed. Review the updated docs/dependency_inventory.md and docs/dependency_inventory.json files before committing.
Policy violations (if any) are recorded in docs/dependency_inventory.violations.json.
Latest check telemetry written to docs/dependency_inventory.telemetry.
MSG
