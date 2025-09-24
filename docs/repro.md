# Reproducible Releases
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

To rebuild official releases byte-for-byte, follow these guidelines.

## Environment
- **Rust toolchain:** pinned via `rust-toolchain.toml` (1.86.0 with `clippy` and `rustfmt`).
- **Base image:** Ubuntu 22.04 or macOS 14 runner.
- **Linker:** `lld` preferred (`apt install lld` on Linux, `brew install llvm` on macOS).
- **Environment variables:**
  ```bash
  export SOURCE_DATE_EPOCH="<unix epoch of tag>"
  export TZ=UTC
  export LC_ALL=C
  export RUSTFLAGS="-C link-arg=-fuse-ld=lld -C link-arg=-Wl,--build-id=sha1"
  ```
  If using nightly you may append `-Zremap-cwd-prefix=$PWD=/src`.

## Build steps
1. `cargo build --release`
2. Strip symbols: `strip --strip-all target/release/node`
3. Create deterministic archive:
   ```bash
   tar --sort=name --owner=0 --group=0 --numeric-owner \
       --mtime=@$SOURCE_DATE_EPOCH -cf node.tar -C target/release node
   gzip -n node.tar
   ```
4. Generate SBOM and provenance:
   ```bash
   scripts/generate_sbom.sh SBOM-x86_64.json
   scripts/release_provenance.sh <tag>
   ```
5. Checksums and signatures:
   ```bash
   sha256sum *.tar.gz SBOM-*.json provenance.json > checksums.txt
   cosign sign-blob --output-signature checksums.txt.sig checksums.txt
   ```

Two independent rebuilds using these inputs should yield identical archives.