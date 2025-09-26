# Reproducible Builds
> **Review (2025-09-25):** Synced Reproducible Builds guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

The project pins the Rust toolchain in `rust-toolchain.toml` and provides `scripts/docker/Dockerfile` that compiles the node with `cargo build --release --locked`.

```
$ docker build -t the-block-repro .
$ docker run --rm the-block-repro cat /usr/local/bin/build.sha256
```

The SHA-256 digest of the binary can be published alongside releases so anyone
can verify their locally built artifact matches the canonical hash.
