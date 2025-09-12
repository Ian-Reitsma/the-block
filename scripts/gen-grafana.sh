#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python monitoring/tools/gen_templates.py monitoring/metrics.json monitoring/grafana
