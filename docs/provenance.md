# Build Provenance
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

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