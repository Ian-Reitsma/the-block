#!/usr/bin/env python3
"""Generate a lightweight telemetry dashboard from the first-party metrics endpoint."""

import json
import pathlib
import sys
import time
from typing import Dict, Tuple
from urllib.error import URLError
from urllib.request import Request, urlopen

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
METRICS_SPEC = json.loads((REPO_ROOT / "monitoring/metrics.json").read_text())
OUTPUT_DIR = REPO_ROOT / "monitoring/output"
OUTPUT_DIR.mkdir(exist_ok=True)

HTML_TEMPLATE = """<!doctype html>
<html lang=\"en\">
  <head>
    <meta charset=\"utf-8\">
    <meta http-equiv=\"refresh\" content=\"{refresh}\">
    <title>The-Block Telemetry Snapshot</title>
    <style>
      body {{ font-family: system-ui, sans-serif; margin: 2rem; background: #0f1115; color: #f8fafc; }}
      h1 {{ margin-bottom: 1rem; }}
      table {{ width: 100%; border-collapse: collapse; margin-bottom: 2rem; }}
      th, td {{ border-bottom: 1px solid #1f2937; padding: 0.5rem 0.75rem; text-align: left; }}
      th {{ text-transform: uppercase; font-size: 0.75rem; letter-spacing: 0.08em; color: #94a3b8; }}
      tr.metric-row:hover {{ background: rgba(148, 163, 184, 0.08); }}
      .status {{ font-weight: 600; }}
      .error {{ color: #fca5a5; }}
    </style>
  </head>
  <body>
    <h1>The-Block Telemetry Snapshot</h1>
    <p class=\"status\">Source: {endpoint}</p>
    {body}
  </body>
</html>
"""

TABLE_TEMPLATE = """<table>
  <thead>
    <tr><th>Metric</th><th>Description</th><th>Value</th></tr>
  </thead>
  <tbody>
    {rows}
  </tbody>
</table>"""

ROW_TEMPLATE = "<tr class=\"metric-row\"><td><code>{name}</code></td><td>{desc}</td><td>{value}</td></tr>"

REFRESH_SECONDS = 5


def load_metrics(endpoint: str) -> Dict[str, float]:
    request = Request(endpoint, headers={"accept": "text/plain"})
    with urlopen(request, timeout=5) as response:
        payload = response.read().decode("utf-8", "replace")
    values: Dict[str, float] = {}
    for line in payload.splitlines():
        if line.startswith("#"):
            continue
        if not line.strip():
            continue
        parts = line.split()
        if len(parts) < 2:
            continue
        name, value = parts[0], parts[-1]
        # Skip histogram helper series
        if name.endswith(("_bucket", "_count", "_sum")):
            continue
        try:
            values[name] = float(value)
        except ValueError:
            continue
    return values


def build_section(title: str, metrics: Tuple[dict, ...], snapshot: Dict[str, float]) -> str:
    rows = []
    for metric in metrics:
        name = metric.get("name", "")
        desc = metric.get("description", "") or "—"
        value = snapshot.get(name)
        if value is None:
            value_display = "<span class=\"error\">missing</span>"
        else:
            value_display = f"{value:g}"
        rows.append(ROW_TEMPLATE.format(name=name, desc=desc, value=value_display))
    if not rows:
        return ""
    body = TABLE_TEMPLATE.format(rows="\n    ".join(rows))
    return f"<h2>{title}</h2>\n{body}"


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: render_foundation_dashboard.py <telemetry-endpoint>", file=sys.stderr)
        return 2
    endpoint = argv[1]
    try:
        snapshot = load_metrics(endpoint)
    except URLError as err:
        body = f'<p class="error">Failed to fetch metrics: {err}</p>'
    except Exception as err:  # pragma: no cover - defensive
        body = f'<p class="error">Unexpected error: {err}</p>'
    else:
        sections = {"DEX": [], "Compute": [], "Gossip": [], "Benchmarks": [], "Other": []}
        for metric in METRICS_SPEC["metrics"]:
            if metric.get("deprecated"):
                continue
            name = metric.get("name", "")
            if name.startswith("dex_"):
                sections["DEX"].append(metric)
            elif name.startswith("compute_") or name.startswith("scheduler_"):
                sections["Compute"].append(metric)
            elif name.startswith("gossip_"):
                sections["Gossip"].append(metric)
            elif name.startswith("benchmark_"):
                sections["Benchmarks"].append(metric)
            else:
                sections["Other"].append(metric)
        rendered = []
        for title, metrics in sections.items():
            section = build_section(title, tuple(metrics), snapshot)
            if section:
                rendered.append(section)
        body = "\n".join(rendered) if rendered else "<p>No metrics available.</p>"
    index = OUTPUT_DIR / "index.html"
    index.write_text(
        HTML_TEMPLATE.format(endpoint=endpoint, body=body, refresh=REFRESH_SECONDS),
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":  # pragma: no cover - script entry
    sys.exit(main(sys.argv))
