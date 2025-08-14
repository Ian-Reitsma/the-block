import importlib
import pathlib
import subprocess
import sys
import os


def _ensure_maturin() -> None:
    """Install maturin if the host environment lacks it."""
    try:
        importlib.import_module("maturin")
    except ModuleNotFoundError:
        subprocess.run(
            [sys.executable, "-m", "pip", "install", "--quiet", "maturin==1.9.2"],
            check=True,
        )


def pytest_sessionstart(session):
    try:
        importlib.import_module("the_block")
    except ModuleNotFoundError:
        repo_root = pathlib.Path(__file__).resolve().parents[1]
        _ensure_maturin()
        env = os.environ.copy()
        env["MATURIN_PYTHON"] = sys.executable
        env["PYO3_PYTHON"] = sys.executable
        subprocess.run(
            [
                sys.executable,
                "-m",
                "maturin",
                "develop",
                "--release",
                "-F",
                "pyo3/extension-module",
                "-F",
                "telemetry",
            ],
            cwd=repo_root,
            check=True,
            env=env,
        )
        venv_site = (
            repo_root
            / ".venv"
            / "lib"
            / f"python{sys.version_info.major}.{sys.version_info.minor}"
            / "site-packages"
        )
        sys.path.append(str(venv_site))
        importlib.import_module("the_block")
