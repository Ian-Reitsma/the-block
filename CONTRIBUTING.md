# Contributing

## CI Settlement Audit

The CI pipeline runs a settlement audit job that executes
`cargo test -p the_block --test settlement_audit --release`.
This test invokes the `settlement.audit` RPC and fails when any
receipt mismatch or missing anchor is detected. Ensure local
changes keep this test green by running:

```bash
cargo test -p the_block --test settlement_audit --release
```

## Fuzz Coverage Prerequisites

Running `scripts/fuzz_coverage.sh` requires `llvm-profdata` and `llvm-cov`.
The script attempts to install these via `rustup component add llvm-tools-preview`
and falls back to the host package manager (`apt`, `brew`, or `pacman`).
If automatic installation fails, install the binaries manually before invoking
the script. Generate `.profraw` files by running fuzz targets with
`RUSTFLAGS="-C instrument-coverage"` and an `LLVM_PROFILE_FILE` path before
invoking the script.

