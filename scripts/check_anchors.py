#!/usr/bin/env python3
"""Validate Markdown references to Rust source line ranges.

Searches repository markdown files for links like ``src/lib.rs#L10-L20`` or
``src/telemetry.rs#L5`` and verifies that the referenced line numbers exist in
the target file. Supports relative paths such as ``../src/lib.rs``.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# ``src/`` prefix with any path ending in ``.rs``
MD_PATTERN = re.compile(
    r"\((?P<path>(?:\./|\../)*src/[A-Za-z0-9_/]+\.rs)#L(?P<start>\d+)(?:-L(?P<end>\d+))?\)"
)


def check_anchor(md_path: Path, match: re.Match[str]) -> str | None:
    rel_path = match.group("path")
    start = int(match.group("start"))
    end = int(match.group("end") or start)

    target = (md_path.parent / rel_path).resolve()
    if not target.exists():
        return f"{md_path}: missing file {rel_path}"

    lines = target.read_text(encoding="utf-8").splitlines()
    total = len(lines)
    if not (1 <= start <= end <= total):
        return (
            f"{md_path}: invalid range {rel_path}#L{start}-L{end} "
            f"(file has {total} lines)"
        )
    return None


def main() -> int:
    errors: list[str] = []
    for md in ROOT.rglob("*.md"):
        if any(part in {"target", ".git", "advisory-db", ".venv"} for part in md.parts):
            continue
        content = md.read_text(encoding="utf-8")
        for match in MD_PATTERN.finditer(content):
            if err := check_anchor(md, match):
                errors.append(err)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("All anchors valid.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
