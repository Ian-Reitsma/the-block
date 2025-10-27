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
`status.snapshot.json`, `status.diff.json`, `overlay.readiness.json`,
`provider.failover.json`, and an `archive/` tree. Each chaos run writes
`archive/latest.json`, a run-scoped `manifest.json` recording the file name,
byte length, and BLAKE3 digest for every artefact, and a deterministic
`run_id.zip` bundle. Optional `--publish-dir`, `--publish-bucket`, and
`--publish-prefix` flags mirror the manifests and bundle into operator-owned
directories or S3-compatible buckets through the first-party
`foundation_object_store` client, which now carries a canonical-request regression
and blocking upload harness proving AWS Signature V4 headers match the published
examples while honouring `TB_CHAOS_ARCHIVE_RETRIES` (minimum 1) and optional
`TB_CHAOS_ARCHIVE_FIXED_TIME` timestamps for reproducible signatures. `scripts/release_provenance.sh` shells out to
`cargo xtask chaos --out-dir releases/v0.1.0/chaos` before hashing artefacts and
fails when the chaos gate trips or when any of those files (including the
archive manifests) are missing so every release proves it passed the provider
failover drills. If `cosign` is installed, the script also attests the checksums
with a SLSA-style provenance. The dependency snapshot captures the
policy-approved crate graph frozen for the release, while the vendor hash pins
the precise source tree that was compiled.

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
monitoring, fails immediately when the `chaos/` artefacts are missing or empty,
and parses `chaos/archive/latest.json` plus the referenced manifest to ensure
every archived file exists, that the recorded bundle size matches the
on-disk `run_id.zip`, and that the manifest’s BLAKE3 digests align with the
files mirrored locally or uploaded to object storage, guaranteeing downstream
consumers inherit the same readiness evidence enforced during release creation.

Run with `cosign` and either `cargo-bom` or `cargo auditable` on your PATH to reproduce SBOMs deterministically (timestamps are fixed via `SOURCE_DATE_EPOCH`).
