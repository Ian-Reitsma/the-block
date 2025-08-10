#!/usr/bin/env python3
"""Check that decoding and re-encoding payload bytes is stable."""

import csv
import subprocess
import sys
from pathlib import Path

import the_block

DEFAULT_PATH = Path("target/serialization_equiv.csv")


def ensure_vectors(path: Path) -> Path:
    if not path.exists():
        subprocess.run(
            ["cargo", "test", "--test", "serialization_equiv", "--quiet"],
            check=True,
        )
    return path


def main(path: Path) -> None:
    target = ensure_vectors(path)
    with target.open(newline="") as fh:
        reader = csv.reader(fh)
        for idx, row in enumerate(reader, start=1):
            raw = bytes.fromhex(row[0])
            payload = the_block.decode_payload(raw)
            out = the_block.canonical_payload(payload)
            if out != raw:
                raise SystemExit(f"mismatch at line {idx}")


if __name__ == "__main__":
    if len(sys.argv) > 2:
        raise SystemExit("usage: serialization_equiv.py [csv-path]")
    path = Path(sys.argv[1]) if len(sys.argv) == 2 else DEFAULT_PATH
    main(path)
