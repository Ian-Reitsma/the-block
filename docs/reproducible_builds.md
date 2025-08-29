# Reproducible Builds

The project pins the Rust toolchain in `rust-toolchain.toml` and provides `scripts/docker/Dockerfile` that compiles the node with `cargo build --release --locked`.

```
$ docker build -t the-block-repro .
$ docker run --rm the-block-repro cat /usr/local/bin/build.sha256
```

The SHA-256 digest of the binary can be published alongside releases so anyone
can verify their locally built artifact matches the canonical hash.
