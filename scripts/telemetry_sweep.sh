#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR=${1:-status}
PROM_URL=${PROM_URL:-http://localhost:9090}

metrics=$("$SCRIPT_DIR/synthetic.sh")
conv=$(curl -s "$PROM_URL/api/v1/query" --data-urlencode 'query=histogram_quantile(0.95, sum(rate(gossip_convergence_seconds_bucket[5m])) by (le))' | jq -r '.data.result[0].value[1] // 0')
fee=$(curl -s "$PROM_URL/api/v1/query" --data-urlencode 'query=CONSUMER_FEE_P90' | jq -r '.data.result[0].value[1] // 0')
comfort=$(curl -s "$PROM_URL/api/v1/query" --data-urlencode 'query=param_change_active{key="ConsumerFeeComfortP90Microunits"}' | jq -r '.data.result[0].value[1] // 0')
ratio=$(curl -s "$PROM_URL/api/v1/query" --data-urlencode 'query=(increase(INDUSTRIAL_DEFERRED_TOTAL[10m])) / clamp_min(increase(INDUSTRIAL_ADMITTED_TOTAL[10m]) + increase(INDUSTRIAL_DEFERRED_TOTAL[10m]), 1)' | jq -r '.data.result[0].value[1] // 0')

success=$(echo "$metrics" | awk '/synthetic_success_total/ {print $2}')
status_color=green
if [ "$success" != "1" ]; then
  status_color=red
elif (( $(echo "$conv > 30" | bc -l) )) || (( $(echo "$fee > $comfort" | bc -l) )) || (( $(echo "$ratio > 0.3" | bc -l) )); then
  status_color=orange
fi

mkdir -p "$OUT_DIR"
cat > "$OUT_DIR/index.html" <<HTML
<html><body style="background-color:$status_color;">
<h1>Telemetry Sweep</h1>
<p>Generated $(date -u)</p>
<pre>$metrics</pre>
<p>Convergence p95: $conv</p>
<p>Consumer fee p90: $fee / comfort $comfort</p>
<p>Industrial deferral ratio: $ratio</p>
</body></html>
HTML
