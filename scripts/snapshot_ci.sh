#!/usr/bin/env bash
set -euo pipefail

TMPDIR=$(mktemp -d)
export TB_SNAPSHOT_INTERVAL=2
export SNAPDIR="$TMPDIR"
PYBIN=".venv/bin/python"
$PYBIN -m pip install maturin >/dev/null 2>&1
$PYBIN -m maturin develop --release -F pyo3/extension-module >/dev/null 2>&1
$PYBIN - <<'PY'
import os, shutil, tempfile
import the_block

path = os.environ['SNAPDIR']

bc = the_block.Blockchain.with_difficulty(path, 0)
for _ in range(3):
    bc.mine_block("miner")
root, _ = bc.account_proof("miner")
restore = tempfile.mkdtemp()
shutil.copytree(path, restore, dirs_exist_ok=True)

del bc

bc2 = the_block.Blockchain.open(restore)
root2, _ = bc2.account_proof("miner")
assert root == root2, f"root mismatch: {root} != {root2}"
metrics = the_block.gather_metrics()
assert "snapshot_fail_total 0" in metrics
assert "snapshot_duration_seconds_count 0" not in metrics
PY
echo "snapshot round-trip verified"
