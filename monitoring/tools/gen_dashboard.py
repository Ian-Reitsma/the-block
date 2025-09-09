#!/usr/bin/env python3
import json, pathlib

repo = pathlib.Path(__file__).resolve().parents[2]
schema = json.loads((repo / 'monitoring/metrics.json').read_text())

panels = []
for i, m in enumerate([m for m in schema["metrics"] if not m.get("deprecated")], 1):
    panels.append({
        "type": "timeseries",
        "title": m["name"],
        "id": i,
        "targets": [{"expr": m["name"]}],
        "options": {"legend": {"showLegend": False}},
        "datasource": {"type": "prometheus", "uid": "prom"},
    })

dashboard = {
    "title": "The-Block Auto",
    "schemaVersion": 37,
    "version": 1,
    "panels": panels,
}

out = repo / 'monitoring/grafana/dashboard.json'
out.write_text(json.dumps(dashboard, indent=2, sort_keys=True) + '\n')
