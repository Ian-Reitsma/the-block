#!/usr/bin/env python3
"""Validate Markdown references to Rust lines and optional section anchors.

Searches repository markdown files for links like ``src/lib.rs#L10-L20`` or
``src/telemetry.rs#L5`` and verifies that the referenced line numbers exist in
the target file. Supports relative paths such as ``../src/lib.rs``.

When invoked with ``--md-anchors`` the script also checks links of the form
``README.md#section`` and ensures that the target heading exists. Section names
are slugified in a GitHub-compatible way: Unicode text is normalized (NFKD),
diacritics are stripped, emoji and other symbols are removed, whitespace and
punctuation collapse to ``-``, and leading or trailing dashes are trimmed.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import functools
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# Rust ``(src|tests|benches|xtask)/*.rs#Lx-Ly`` anchors
# Allow dots and nested module paths; the heavy lifting is done by
# ``check_rust_anchor`` which resolves the path and verifies the line range.
RUST_PATTERN = re.compile(
    r"\((?P<path>(?:\./|\../)*(?:src|tests|benches|xtask)/[A-Za-z0-9_.\-/]+\.rs)#L(?P<start>\d+)(?:-L(?P<end>\d+))?\)"
)

# Markdown ``file.md#section`` anchors
MD_PATTERN = re.compile(
    r"\((?P<path>(?:\./|\../)*[A-Za-z0-9_/]+\.md)#(?P<section>[A-Za-z0-9_-]+)\)"
)


@functools.lru_cache(maxsize=None)
def _cached_lines(path: Path) -> list[str]:
    return path.read_text(encoding="utf-8").splitlines()


def check_rust_anchor(md_path: Path, match: re.Match[str]) -> str | None:
    rel_path = match.group("path").replace("\\", "/")
    start = int(match.group("start"))
    end = int(match.group("end") or start)

    target = (md_path.parent / rel_path).resolve()
    if not target.exists():
        return f"{md_path}: missing file {rel_path}"

    lines = _cached_lines(target)
    total = len(lines)
    if not (1 <= start <= end <= total):
        return (
            f"{md_path}: invalid range {rel_path}#L{start}-L{end} "
            f"(file has {total} lines)"
        )
    return None


def slugify(text: str) -> str:
    """Produce a GitHub-style slug for a heading text.

    The algorithm mirrors GitHub's anchor generation: Unicode is normalized,
    diacritics are removed, emoji/symbol characters are dropped, whitespace and
    punctuation collapse to a single ``-``, and any leading or trailing dashes
    are trimmed.
    """

    import unicodedata

    normalized = unicodedata.normalize("NFKD", text)
    normalized = "".join(ch for ch in normalized if not unicodedata.combining(ch))

    slug: list[str] = []
    dash = False
    for ch in normalized:
        if ch.isalnum():
            slug.append(ch.lower())
            dash = False
        else:
            category = unicodedata.category(ch)
            if category[0] in {"Z", "P"}:
                if not dash:
                    slug.append("-")
                    dash = True
            # Drop any other category without inserting a dash so emoji vanish
            # without leaving extra separators.

    return "".join(slug).strip("-")


def check_md_anchor(md_path: Path, match: re.Match[str]) -> str | None:
    rel_path = match.group("path").replace("\\", "/")
    section = match.group("section")
    target = (md_path.parent / rel_path).resolve()
    if not target.exists():
        return f"{md_path}: missing file {rel_path}"
    content = target.read_text(encoding="utf-8")
    anchors = {
        slugify(line.lstrip("#"))
        for line in content.splitlines()
        if line.startswith("#")
    }
    anchors.update(
        m.group(1).lower() for m in re.finditer(r"<a id=\"([A-Za-z0-9_-]+)\"", content)
    )
    section_slug = slugify(section)
    if section_slug not in anchors and section.lower() not in anchors:
        return f"{md_path}: missing anchor {rel_path}#{section}"
    return None


def _process_md(md: Path, md_anchors: bool) -> list[str]:
    errors: list[str] = []
    content = md.read_text(encoding="utf-8").replace("\\", "/")
    for match in RUST_PATTERN.finditer(content):
        if err := check_rust_anchor(md, match):
            errors.append(err)
    if md_anchors:
        for match in MD_PATTERN.finditer(content):
            if err := check_md_anchor(md, match):
                errors.append(err)
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--md-anchors",
        action="store_true",
        help="Validate Markdown #section anchors in addition to Rust line anchors",
    )
    args = parser.parse_args()

    md_files = [
        md
        for md in ROOT.rglob("*.md")
        if not any(
            part in {"target", ".git", "advisory-db", ".venv"} for part in md.parts
        )
    ]
    errors: list[str] = []
    with concurrent.futures.ThreadPoolExecutor() as ex:
        for result in ex.map(lambda m: _process_md(m, args.md_anchors), md_files):
            errors.extend(result)

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("All anchors valid.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
