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

## Documentation

Building the documentation requires [`mdbook`](https://rust-lang.github.io/mdBook/).
Install it with `cargo install mdbook` and verify the book renders cleanly via

```bash
mdbook build docs
```

Continuous integration runs this command for every pull request, so ensure it
passes locally before submitting patches.

## Agent/Codex Workflow

Contributors using AI agents or codex-style tooling must read and honor `AGENTS.md` at the repository root. The file defines coding standards, testing requirements, and commit protocol. Agents should describe changes in the pull request summary and run `cargo test --all --features test-telemetry --release` before submitting.

Install the provided `scripts/pre-commit.sample` as a Git hook to automatically run `just format` and `just lint` prior to every commit:

```bash
ln -s ../../scripts/pre-commit.sample .git/hooks/pre-commit
```

## Branch Strategy

- Fork or create a feature branch off `main` for all changes.
- Rebase against `main` before opening a pull request; avoid merge commits.
- Squash merges are performed by maintainers; do not force-push after review starts.

