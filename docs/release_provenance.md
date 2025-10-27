# Release Provenance & SBOM
> **Review (2025-09-25):** Synced Release Provenance & SBOM guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Each tagged release includes a Software Bill of Materials (SBOM) and a signed provenance statement.

## Generating Artifacts

```bash
scripts/release_provenance.sh v0.1.0
```

This produces `releases/v0.1.0/` containing the built binaries, `SBOM-x86_64.json`,
`dependency-snapshot.json`, `vendor-sha256.txt`, `checksums.txt`, and
`provenance.json`, alongside a `chaos/` directory holding
`status.snapshot.json`, `status.diff.json`, `overlay.readiness.json`, and
`provider.failover.json`. `scripts/release_provenance.sh` shells out to
`cargo xtask chaos --out-dir releases/v0.1.0/chaos` before hashing artefacts and
fails when the chaos gate trips or when any of those files are missing so every
release proves it passed the provider failover drills. If `cosign` is installed,
the script also attests the checksums with a SLSA-style provenance. The
dependency snapshot captures the policy-approved crate graph frozen for the
release, while the vendor hash pins the precise source tree that was compiled.

## Verifying

```bash
scripts/verify_release.sh releases/v0.1.0/node-x86_64-unknown-linux-gnu.tar.gz \
  releases/v0.1.0/checksums.txt releases/v0.1.0/checksums.txt.sig
```

The script checks SHA-256 hashes, verifies the cosign attestation when
available, and compares the published dependency snapshot against
`docs/dependency_inventory.json`. If the snapshot diverges, a warning is emitted
so operators can scrutinise the policy changes before upgrading. The script also
reports the vendor-tree digest captured in `checksums.txt` for out-of-band
monitoring and fails immediately when the `chaos/` artefacts are missing or
empty, ensuring downstream consumers inherit the same readiness gate enforced
during release creation.

Run with `cosign` and either `cargo-bom` or `cargo auditable` on your PATH to reproduce SBOMs deterministically (timestamps are fixed via `SOURCE_DATE_EPOCH`).
