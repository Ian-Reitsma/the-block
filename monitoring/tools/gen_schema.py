#!/usr/bin/env python3
import json, re, pathlib

repo = pathlib.Path(__file__).resolve().parents[2]
telemetry = repo / 'node/src/telemetry.rs'
schema_path = repo / 'monitoring/metrics.json'

text = telemetry.read_text()

# metrics with label vectors
vec_pattern = re.compile(
    r"::new\(\s*Opts::new\(\s*\"([^\"]+)\"\s*,\s*\"([^\"]+)\"\)\s*,\s*&\[(.*?)\]\s*,?\s*\)",
    re.DOTALL,
)
metrics = {}
for name, desc, labels in vec_pattern.findall(text):
    lbls = [l.strip().strip('\"') for l in labels.split(',') if l.strip()]
    metrics[name] = {"description": desc, "labels": lbls}

# simple metrics without label vectors
simple_pattern = re.compile(
    r"::new\(\s*\"([^\"]+)\"\s*,\s*\"([^\"]+)\"\s*,?\s*\)",
    re.DOTALL,
)
for name, desc in simple_pattern.findall(text):
    metrics.setdefault(name, {"description": desc, "labels": []})

existing = {}
if schema_path.exists():
    existing = {m["name"]: m for m in json.loads(schema_path.read_text()).get("metrics", [])}

# handle deprecations
for name in list(existing):
    if name not in metrics:
        m = existing[name]
        m["deprecated"] = True
        metrics[name] = {"description": m.get("description", ""), "labels": m.get("labels", []), "deprecated": True}

output = {"metrics": []}
for name in sorted(metrics):
    meta = metrics[name]
    output["metrics"].append({
        "name": name,
        "description": meta.get("description", ""),
        "labels": meta.get("labels", []),
        "deprecated": bool(meta.get("deprecated", False)),
    })

schema_path.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")
