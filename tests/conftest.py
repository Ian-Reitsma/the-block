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


def _build_extension() -> None:
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


def _ensure_extension() -> None:
    try:
        importlib.import_module("the_block")
    except ModuleNotFoundError:
        _build_extension()
        importlib.import_module("the_block")


_ensure_extension()


def pytest_sessionstart(session):
    _ensure_extension()
