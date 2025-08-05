import importlib
import pathlib
import subprocess
import sys
import os


def pytest_sessionstart(session):
    try:
        importlib.import_module("the_block")
    except ModuleNotFoundError:
        repo_root = pathlib.Path(__file__).resolve().parents[1]
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
