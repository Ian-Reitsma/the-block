# Release Provenance & SBOM
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Each tagged release includes a Software Bill of Materials (SBOM) and a signed provenance statement.

## Generating Artifacts

```bash
scripts/release_provenance.sh v0.1.0
```

This produces `releases/v0.1.0/` containing the built binaries, `SBOM-x86_64.json`, `checksums.txt`, and `provenance.json`. If `cosign` is installed, the script also attests the checksums with a SLSA-style provenance.

## Verifying

```bash
scripts/verify_release.sh releases/v0.1.0
```

The script checks SHA-256 hashes and verifies the cosign attestation when available.

Run with `cosign` and either `cargo-bom` or `cargo auditable` on your PATH to reproduce SBOMs deterministically (timestamps are fixed via `SOURCE_DATE_EPOCH`).