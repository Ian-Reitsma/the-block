# Supply Chain Security
> **Review (2025-09-25):** Synced Supply Chain Security guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This project enables reproducible builds via Nix. The `nix` directory contains a `build.nix` that pins dependencies and produces deterministic binaries. Operators can compare hashes using `nix-build` on multiple machines.

Each release generates a CycloneDX SBOM and container images are signed with `cosign`. CI runs `cargo audit` and `honggfuzz` to detect dependency issues and protocol bugs.

To verify artifacts independently, rebuild using Nix and compare the SHA256 of the produced binaries with the published values. The `scripts/verify_image.sh` script checks that signed images match the expected digest, allowing third parties to validate releases without trusting the CI.

## Dependency Governance Registry

The `tools/dependency_registry` binary inventories every crate resolved in `Cargo.lock`, assigns a risk tier, and records origin and license metadata. Running

```bash
cargo run -p dependency_registry
```

generates:

- `target/dependency-registry.json` – machine-readable snapshot of the full dependency DAG.
- `docs/dependency_inventory.md` – human-readable table sorted by tier and crate name.
- `target/dependency-violations.json` – policy violations such as excessive depth or forbidden licenses.

A committed baseline lives at `docs/dependency_inventory.json`. Use `cargo run -p dependency_registry -- --check` in CI or locally to ensure the freshly generated registry matches that baseline and that no violations slipped in. When the dependency graph legitimately changes, refresh the artifacts via:

```bash
./scripts/dependency_snapshot.sh
```

This helper runs the registry, copies the JSON artifacts into `docs/`, and reminds you to inspect diffs before committing. Always review the resulting Markdown/JSON changes in Git to confirm the new crates and tiers are expected.

To investigate differences between two snapshots, use `cargo run -p dependency_registry -- --diff <old.json> <new.json>`. The command prints added, removed, or modified crates with tier, origin, and license changes. Developers can also audit a single crate with `cargo run -p dependency_registry -- --explain <crate-name>`, which renders its metadata, direct dependencies, and dependents.

Waivers or tier adjustments are captured in `config/dependency_policies.toml`. Update the `[tiers]` lists to reclassify crates and adjust `[licenses]` or `[settings.max_depth]` to tune enforcement. Every waiver should be documented in the same commit so reviewers understand why the policy changed.

## Tooling & CI Integration

- `just dependency-audit`, `make dependency-check`, and the "Dependency policy check"
  job in `.github/workflows/ci.yml` all invoke the registry in `--check` mode and
  block drift before tests run.
- Release scripts (`scripts/release_provenance.sh`, `scripts/verify_release.sh`)
  vendor the dependency tree, hash the result, and archive the registry snapshot
  alongside provenance metadata so downstream users can verify reproducibility.
- Developers can opt into the sample Git hook at `config/hooks/pre-commit` to
  run the audit locally before crafting commits.

## Pivot Alignment

The dependency-sovereignty roadmap in
[`docs/pivot_dependency_strategy.md`](pivot_dependency_strategy.md) governs the
replacement order for third-party crates. Wrapper crates (runtime, transport,
overlay, storage engine, coding, crypto, codec) must be used instead of direct
imports, and telemetry/CLI now surface active backends so operators can observe
rollouts. Governance parameters already gate backend selection; the registry,
tooling, and release steps here keep clusters aligned with those policies while
CI and provenance scripts refuse tags when dependency snapshots drift.
