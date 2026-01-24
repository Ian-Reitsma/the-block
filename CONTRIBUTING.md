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
the script. The command now defaults to running `cargo fuzz run compute_market`
(with `RUSTFLAGS="-C instrument-coverage"`) and recording `LLVM_PROFILE_FILE`
values under `fuzz/coverage/profraw`. Use `--target` / `--targets` to add other
targets, `--duration` to bound their `-max_total_time`, or `--no-run` when you
only want to merge pre-existing `.profraw` artifacts. Specify `OUT_DIR` as the
first argument to move the coverage HTML and profdata files elsewhere.

## Documentation

Building the documentation requires [`mdbook`](https://rust-lang.github.io/mdBook/).
Install it with `cargo install mdbook` and verify the book renders cleanly via

```bash
mdbook build docs
```

Continuous integration runs this command for every pull request, so ensure it
passes locally before submitting patches.

## Managing dependencies

The dependency policy for the workspace lives in
[`config/dependency_policies.toml`](config/dependency_policies.toml).  It
defines the maximum allowed dependency depth, the risk tier for crates that are
strategic or replaceable, and licenses that are forbidden in downstream
transitive dependencies.

Run the registry auditor locally before committing changes:

```bash
cargo run -p dependency_registry -- --check config/dependency_policies.toml
```

The same command is available through `just dependency-audit` and `make
dependency-check`.  The tool produces `target/dependency-registry.json` and
`target/dependency-violations.json`, and it refreshes the committed baseline in
`docs/dependency_inventory.json`/`.md`.  Continuous integration surfaces the
registry snapshot as a build artifact and fails when unapproved crates are
introduced.

To request an exception, open a pull request that updates
`config/dependency_policies.toml` with the proposed tier or license change and
document the rationale in the PR description.  Include the regenerated registry
artifacts so reviewers can verify the impact of the policy change.  Developers
who want the audit to run automatically can symlink
`config/hooks/pre-commit` into `.git/hooks/pre-commit`.

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
