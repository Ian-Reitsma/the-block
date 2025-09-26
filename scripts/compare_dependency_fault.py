#!/usr/bin/env python3
"""Compare two dependency fault simulation runs."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Dict, Tuple

Metrics = Dict[str, float]
ScenarioKey = Tuple[str, int]


def load_metrics(root: Path) -> Dict[ScenarioKey, Metrics]:
    reports: Dict[ScenarioKey, Metrics] = {}
    for metrics_file in root.rglob("metrics.json"):
        with metrics_file.open("r", encoding="utf-8") as fh:
            data = json.load(fh)
        key = (data["scenario"], int(data["iteration"]))
        reports[key] = data
    if not reports:
        raise SystemExit(f"no metrics.json files found under {root}")
    return reports


def diff_metrics(old: Metrics, new: Metrics) -> Dict[str, float]:
    interesting = [
        "transport_failures",
        "overlay_failures",
        "storage_failures",
        "coding_bytes",
        "coding_failures",
        "crypto_failures",
        "codec_failures",
        "rpc_latency_ms",
        "receipts_persisted",
    ]
    delta = {}
    for key in interesting:
        old_val = float(old.get(key, 0))
        new_val = float(new.get(key, 0))
        delta[key] = new_val - old_val
    return delta


def render_markdown(old_root: Path, new_root: Path, changes: Dict[ScenarioKey, Dict[str, float]]) -> str:
    lines = ["# Dependency Fault Comparison", ""]
    lines.append(f"Old run: `{old_root}`  ")
    lines.append(f"New run: `{new_root}`  ")
    lines.append("")
    for (scenario, iteration), metrics in sorted(changes.items()):
        lines.append(f"## {scenario} (iteration {iteration})")
        lines.append("| Metric | Δ |")
        lines.append("| --- | ---: |")
        for metric, delta in sorted(metrics.items()):
            lines.append(f"| {metric} | {delta:+.3f} |")
        lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("old", type=Path, help="baseline dependency fault run directory")
    parser.add_argument("new", type=Path, help="comparison dependency fault run directory")
    parser.add_argument("--output", type=Path, help="optional markdown output path")
    args = parser.parse_args()

    old_reports = load_metrics(args.old)
    new_reports = load_metrics(args.new)
    changes: Dict[ScenarioKey, Dict[str, float]] = {}

    for key, new_metrics in new_reports.items():
        if key not in old_reports:
            continue
        changes[key] = diff_metrics(old_reports[key], new_metrics)

    markdown = render_markdown(args.old, args.new, changes)
    if args.output:
        args.output.write_text(markdown, encoding="utf-8")
    else:
        print(markdown)


if __name__ == "__main__":
    main()
