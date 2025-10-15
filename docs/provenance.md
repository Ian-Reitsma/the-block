# Build Provenance
> **Review (2025-09-25):** Synced Build Provenance guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This document outlines the supply-chain assumptions and provenance checks.

## Overview
- Each binary embeds the git commit hash at build time.
- On startup, nodes hash their own executable and compare it against the
  embedded value, incrementing `build_provenance_valid_total` or
  `build_provenance_invalid_total`.
- The CLI exposes `version provenance` to display the expected and actual
  hash of the running binary.
- Offline verification can be performed with `tools/provenance-verify` by
  supplying the artifact path and expected hash.
- CI generates CycloneDX SBOMs via `cargo auditable` and signs release
  artifacts with cosign; signatures are stored under `release/` and verified at
  startup.

## Dependency freezes and vendor attestations

The release provenance pipeline now freezes both the dependency registry and
the vendored source tree before any binaries are built:

- `scripts/release_provenance.sh` invokes
  `cargo run -p dependency_registry -- --check --snapshot` to emit
  `dependency-snapshot.json`, a canonicalised view of the policy-compliant
  dependency graph that ships alongside the binaries. The snapshot hash is
  embedded in `provenance.json` and appears in `checksums.txt` so downstream
  auditors can verify that their local policy baseline matches the published
  release.
- The same run writes `dependency-check.telemetry`,
  `dependency-check.summary.json`, and `dependency-metrics.telemetry`, hashing
  each artefact into `provenance.json`/`checksums.txt` so release consumers see
  the drift verdict and metrics that backed the promotion decision.
- The same script runs `cargo vendor` into a staging directory, normalises the
  tree with sorted metadata, and records the resulting SHA-256 digest in both
  `vendor-sha256.txt` and the provenance record. Consumers can compare the
  digest against a locally generated vendor tree to confirm there were no
  supply-chain substitutions between the policy check and the final build.
- `tools/vendor_sync` streamlines updates to the committed `vendor/` directory
  and supports a `--check` mode used by CI to guarantee that the workspace is
  already in sync before the release job starts. This keeps reproducible builds
  honest by failing fast whenever an engineer forgets to vendor a new
  third-party crate.

When automation is unavailable (for example, during an incident drill), the
freeze can be reproduced manually:

```bash
tools/vendor_sync --check              # ensure the tree matches the manifest
cargo vendor --locked /tmp/vendor      # capture the frozen vendor tree
tar --sort=name --owner=0 --group=0 --numeric-owner -cf - /tmp/vendor | sha256sum
cargo run -p dependency_registry -- --check \
  --snapshot releases/manual/dependency-snapshot.json config/dependency_policies.toml
sha256sum releases/manual/dependency-snapshot.json
```

Record both digests in your change-management system and attach them to any
out-of-band release notes so reviewers can cross-check policy adherence.

Operations monitors the vendor digest published in `checksums.txt` via external
watchers (e.g., the provenance dashboard and the checksum mirror). Any drift is
alerted immediately, prompting a regression sweep of the dependency registry
before binaries are promoted.

Release managers should also capture governance-backed backend switches in the
release notes. Run

```bash
cargo run -p release_notes -- --state-dir /var/lib/the-block \
  --since-epoch <last_release_epoch>
```

or point `--history` at the archived `governance/history/dependency_policy.json`
file exported from staging. The command emits Markdown-ready bullets describing
runtime, transport, and storage policy updates so downstream operators can audit
the rollout alongside dependency snapshots and vendor hashes.

## Release fetch and attestation

Governance-secured releases extend provenance checks beyond build-time hashes.
`the_block::update::fetch_release` downloads artifacts from
`$TB_RELEASE_SOURCE_URL`, verifies the BLAKE3 digest, and enforces any
configured Ed25519 attestor signatures before staging the binary. When
`--install` is supplied, `install_release` calls `ensure_release_authorized` so
the install is recorded via `release_installs_total`. Startup failures trigger
`update::rollback_failed_startup`, restoring the previous binary before the node
reboots.

Multi-signature approvals store the signer roster and threshold alongside each
release. During submission, `controller::submit_release` rejects attestations
that are not part of the configured signer snapshot, and `provenance::verify`
normalizes signatures over `"release:<hash>"` so explorers and operators can
audit the same bytes. The signer snapshot and threshold surface through
`gov.release_signers`, while install timestamps are exposed via the explorer
`/releases` endpoint and CLI history tooling. These records allow fleets to
prove both build provenance and rollout coverage during audits.
