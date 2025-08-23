# Developer Setup

Run `./bootstrap.sh` to install toolchains. The script creates `.venv` and prepends its `bin` directory to `PATH`, so `python demo.py` works immediately. If a system `python` is missing, a shim is installed at `bin/python` and added to the path.

After bootstrapping, `just demo` runs the same walkthrough without manually
activating the environment. Sample compute workloads under
`examples/workloads/` can be exercised with:

```bash
cargo run --example run_workload examples/workloads/inference.slice
```

## Installing nextest

The test suite uses [`cargo nextest`](https://nexte.st). If your host toolchain
is older than the minimum required to build it, fetch a prebuilt binary:

```bash
curl -L https://get.nexte.st/latest/linux | tar xz
mv cargo-nextest ~/.cargo/bin/
```

Running `cargo nextest --version` should then report a matching release.

## Troubleshooting libpython

Rust tests dynamically link against the Python shared library. If you see errors like `libpython3.*.so: cannot open shared object file`, install the Python development package (e.g. `sudo apt-get install python3.12-dev`) and ensure the library directory from `python3-config --ldflags` is present in `LD_LIBRARY_PATH` (or `DYLD_LIBRARY_PATH` on macOS).
