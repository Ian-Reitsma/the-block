#!/usr/bin/env python3
import json, re, pathlib

repo = pathlib.Path(__file__).resolve().parents[2]
telemetry = repo / 'node/src/telemetry.rs'
text = telemetry.read_text()
metrics = re.findall(r'Opts::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"', text)
panels = []
for i, (name, desc) in enumerate(metrics, 1):
    panels.append({
        'type': 'timeseries',
        'title': name,
        'id': i,
        'targets': [{'expr': name}],
        'options': {'legend': {'showLegend': False}},
        'datasource': {'type': 'prometheus', 'uid': 'prom'},
    })

dashboard = {
    'title': 'The-Block Auto',
    'schemaVersion': 37,
    'version': 1,
    'panels': panels,
}
out = repo / 'monitoring/grafana/dashboard.json'
out.write_text(json.dumps(dashboard, indent=2) + '\n')
