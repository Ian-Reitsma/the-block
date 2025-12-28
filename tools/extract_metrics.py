#!/usr/bin/env python3
"""
Utility to extract the Prometheus metrics declared in node/src/telemetry.rs.

Examples:
    python tools/extract_metrics.py --format markdown
    python tools/extract_metrics.py --format json > metrics.json
"""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Sequence

TELEMETRY_PATH = Path("node/src/telemetry.rs")


@dataclass
class Metric:
    name: str
    kind: str
    desc: str
    labels: Sequence[str]

    def units(self) -> str:
        name = self.name.lower()
        desc = self.desc.lower()
        if name.endswith("_seconds") or " seconds" in desc or "latency" in desc or "duration" in desc:
            return "seconds"
        if name.endswith("_ms") or "milliseconds" in desc or " ms" in desc:
            return "milliseconds"
        if "microsecond" in desc or "micros" in desc or name.endswith("_micros"):
            return "microseconds"
        if name.endswith("_bytes") or "bytes" in desc:
            return "bytes"
        if name.endswith("_ratio") or "ratio" in desc:
            return "ratio"
        if name.endswith("_ppm") or "ppm" in desc:
            return "ppm"
        if name.endswith("_percent") or "percent" in desc:
            return "percent"
        block_metrics = {
            "base_reward",
            "rent_escrow_burned_total",
            "rent_escrow_locked_total",
            "rent_escrow_refunded_total",
            "slashing_burn_total",
            "receipt_settlement_storage",
            "receipt_settlement_compute",
            "receipt_settlement_energy",
            "receipt_settlement_ad",
            "energy_settlement_total",
            "energy_treasury_fee_total",
            "dns_stake_locked",
        }
        if (
            name in block_metrics
            or name.endswith("_block")
            or name.endswith("_reward")
            or name.endswith("_rebate")
            or "settlement" in name
            or "treasury" in name
            or name.startswith("rent_escrow")
            or name.startswith("dns_stake")
            or "stake" in name
            or "burn" in name
            or "block" in desc
        ):
            return "BLOCK"
        if "usd" in desc:
            return "USD-equivalent"
        if name.endswith("_total") or "total" in desc or "count" in desc:
            return "count"
        return "unitless"

    def markdown_row(self) -> str:
        labels = ", ".join(self.labels) if self.labels else "–"
        safe_desc = self.desc.replace("|", r"\|")
        return f"| `{self.name}` | {self.kind} | {labels} | {self.units()} | {safe_desc} |"


def _label_list(raw: str | None) -> List[str]:
    if not raw:
        return []
    labels: List[str] = []
    for part in raw.split(","):
        part = part.strip()
        if not part:
            continue
        if part.startswith('"') and part.endswith('"'):
            part = part[1:-1]
        labels.append(part)
    return labels


def _collect_metrics(source: str) -> List[Metric]:
    events = []
    flags = re.MULTILINE | re.DOTALL
    patterns = [
        ("IntGauge", re.compile(r'IntGauge::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,?', flags)),
        ("IntCounter", re.compile(r'IntCounter::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,?', flags)),
        (
            "IntGaugeVec",
            re.compile(r'IntGaugeVec::new\(\s*Opts::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*\)\s*,\s*&\[([^\]]*?)\]\s*\)', flags),
        ),
        (
            "IntCounterVec",
            re.compile(r'IntCounterVec::new\(\s*Opts::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*\)\s*,\s*&\[([^\]]*?)\]\s*\)', flags),
        ),
        (
            "GaugeVec",
            re.compile(r'GaugeVec::new\(\s*Opts::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*\)\s*,\s*&\[([^\]]*?)\]\s*\)', flags),
        ),
        (
            "HistogramInline",
            re.compile(r'Histogram::with_opts\(\s*HistogramOpts::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*(.*?)\)', flags),
        ),
        (
            "HistogramVecInline",
            re.compile(
                r'HistogramVec::new\(\s*HistogramOpts::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*(.*?)\)\s*,\s*&\[([^\]]*?)\]\s*\)',
                flags,
            ),
        ),
        (
            "HistogramOpts",
            re.compile(
                r'let\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*HistogramOpts::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*(.*?)\);',
                flags,
            ),
        ),
        (
            "HistogramWithVar",
            re.compile(r'Histogram::with_opts\(\s*(?!HistogramOpts::new)([A-Za-z_][A-Za-z0-9_]*)\s*\)', flags),
        ),
        (
            "HistogramVecWithVar",
            re.compile(
                r'HistogramVec::new\(\s*(?!HistogramOpts::new)([A-Za-z_][A-Za-z0-9_]*)\s*,\s*&\[([^\]]*?)\]\s*\)',
                flags,
            ),
        ),
    ]
    for kind, pattern in patterns:
        for match in pattern.finditer(source):
            events.append((match.start(), kind, match))
    events.sort(key=lambda item: item[0])

    var_map: Dict[str, tuple[str, str]] = {}
    metrics: Dict[str, Metric] = {}

    def ensure(name: str, kind: str, desc: str, labels: Iterable[str] = ()):
        if name not in metrics:
            metrics[name] = Metric(name=name, kind=kind, desc=desc, labels=list(labels))

    for _, kind, match in events:
        if kind == "HistogramOpts":
            var_map[match.group(1)] = (match.group(2), match.group(3))
            continue
        if kind == "HistogramInline":
            ensure(match.group(1), "Histogram", match.group(2))
            continue
        if kind == "HistogramVecInline":
            ensure(match.group(1), "HistogramVec", match.group(2), _label_list(match.group(4)))
            continue
        if kind == "HistogramWithVar":
            var = match.group(1)
            if var in var_map:
                ensure(var_map[var][0], "Histogram", var_map[var][1])
            continue
        if kind == "HistogramVecWithVar":
            var = match.group(1)
            if var in var_map:
                ensure(var_map[var][0], "HistogramVec", var_map[var][1], _label_list(match.group(2)))
            continue
        if kind in {"IntGauge", "IntCounter"}:
            ensure(match.group(1), kind, match.group(2))
            continue
        if kind in {"IntGaugeVec", "IntCounterVec", "GaugeVec"}:
            ensure(match.group(1), kind, match.group(2), _label_list(match.group(3)))
            continue

    return [metrics[name] for name in sorted(metrics)]


def _format_markdown(metrics: Sequence[Metric]) -> str:
    lines = [
        "| Name | Type | Labels | Units | Description |",
        "| --- | --- | --- | --- | --- |",
    ]
    for metric in metrics:
        lines.append(metric.markdown_row())
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--telemetry", default=str(TELEMETRY_PATH), help="Path to telemetry.rs")
    parser.add_argument("--format", choices=("markdown", "json"), default="markdown")
    args = parser.parse_args()

    source = Path(args.telemetry).read_text()
    metrics = _collect_metrics(source)

    if args.format == "json":
        print(
            json.dumps(
                [
                    {
                        "name": metric.name,
                        "type": metric.kind,
                        "labels": list(metric.labels),
                        "units": metric.units(),
                        "description": metric.desc,
                    }
                    for metric in metrics
                ],
                indent=2,
            )
        )
        return

    print(_format_markdown(metrics))


if __name__ == "__main__":
    main()
