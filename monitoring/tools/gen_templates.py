#!/usr/bin/env python3
import json, pathlib

repo = pathlib.Path(__file__).resolve().parents[2]
dash = json.loads((repo / 'monitoring/grafana/dashboard.json').read_text())
for tpl_path in (repo / 'monitoring/templates').glob('*.json'):
    tpl = json.loads(tpl_path.read_text())
    merged = dash.copy()
    merged.update(tpl)
    out = repo / 'monitoring/grafana' / f"{tpl_path.stem}.json"
    out.write_text(json.dumps(merged, indent=2, sort_keys=True) + '\n')
