# Build Provenance

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
