#!/usr/bin/env python3
import argparse
import json
import os
import platform
import subprocess
import time
from pathlib import Path


def collect_metadata() -> dict:
    commit = (
        subprocess.check_output(["git", "rev-parse", "--short", "HEAD"])
        .decode()
        .strip()
    )
    orchard_env = {k: v for k, v in os.environ.items() if k.startswith("ORCHARD_")}
    return {
        "commit": commit,
        "system": platform.platform(),
        "processor": platform.processor(),
        "orchard_env": orchard_env,
    }


def detect_backend() -> str:
    if platform.system() != "Darwin" or os.environ.get("ORCHARD_FORCE_CPU") == "1":
        return "cpu"
    return "metal"


def collect_profile() -> list[str]:
    log = Path("/tmp/orchard_tensor_profile.log")
    if log.exists():
        return log.read_text().splitlines()
    return []


def run_kernel(binary: Path, kernel: str, args: list[str]) -> dict:
    cmd = [str(binary), kernel] + args
    seconds = float(subprocess.check_output(cmd).decode().strip())
    return {"kernel": kernel, "seconds": seconds}


def main() -> None:
    parser = argparse.ArgumentParser(description="Benchmark Tensor v0 kernels")
    parser.add_argument(
        "-o", "--out", default="benchmarks/results", help="output directory"
    )
    opts = parser.parse_args()

    bench_bin = Path("build/benchmarks/orchard_bench")
    if not bench_bin.exists():
        raise FileNotFoundError(
            f"{bench_bin} missing; run 'cmake --build build --target orchard_bench'"
        )
    kernels = [
        ("add", ["1000000"]),
        ("mul", ["1000000"]),
        ("matmul", ["64", "64", "64"]),
        ("reduce_sum", ["1000000"]),
        ("mean", ["1000000"]),
        ("transpose", ["1024", "1024"]),
    ]

    if os.getenv("ORCHARD_TENSOR_PROFILE"):
        Path("/tmp/orchard_tensor_profile.log").unlink(missing_ok=True)

    results = [run_kernel(bench_bin, k, args) for k, args in kernels]
    meta = collect_metadata()
    meta["backend"] = detect_backend()
    profile = collect_profile() if os.getenv("ORCHARD_TENSOR_PROFILE") else []

    out_root = Path(opts.out)
    commit_dir = out_root / meta["commit"]
    commit_dir.mkdir(parents=True, exist_ok=True)
    out_file = commit_dir / "benchmarks.json"
    if out_file.exists():
        data = json.loads(out_file.read_text())
    else:
        data = {"metadata": meta, "runs": []}
    data["runs"].append(
        {"benchmarks": results, "timestamp": time.time(), "profile": profile}
    )
    out_file.write_text(json.dumps(data, indent=2))

    print(json.dumps({"metadata": meta, "benchmarks": results}, indent=2))


if __name__ == "__main__":
    main()
