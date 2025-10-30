#!/usr/bin/env python3
import json, pathlib

repo = pathlib.Path(__file__).resolve().parents[2]
schema = json.loads((repo / 'monitoring/metrics.json').read_text())

groups = {"DEX": [], "Compute": [], "Gossip": [], "Benchmarks": [], "Other": []}
for m in [m for m in schema["metrics"] if not m.get("deprecated")]:
    panel = {
        "type": "timeseries",
        "title": m["name"],
        "targets": [{"expr": m["name"]}],
        "options": {"legend": {"showLegend": False}},
        "datasource": {"type": "foundation-telemetry", "uid": "foundation"},
    }
    name = m["name"]
    if name.startswith("dex_"):
        groups["DEX"].append(panel)
    elif name.startswith("compute_") or name.startswith("scheduler_"):
        groups["Compute"].append(panel)
    elif name.startswith("gossip_"):
        groups["Gossip"].append(panel)
    elif name.startswith("benchmark_"):
        groups["Benchmarks"].append(panel)
    else:
        groups["Other"].append(panel)

panels = []
pid = 1
for title, ps in groups.items():
    if not ps:
        continue
    panels.append({"type": "row", "title": title, "id": pid})
    pid += 1
    for p in ps:
        p["id"] = pid
        pid += 1
        panels.append(p)

dashboard = {
    "title": "The-Block Auto",
    "schemaVersion": 37,
    "version": 1,
    "panels": panels,
}

out = repo / 'monitoring/grafana/dashboard.json'
out.write_text(json.dumps(dashboard, indent=2, sort_keys=True) + '\n')
