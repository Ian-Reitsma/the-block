# Commit–Reveal Scheme
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Certain protocol paths require participants to commit to a value before the
network reveals it. The `commit_reveal` module offers a lightweight helper that
supports both post‑quantum and classical hash-based commits.

## Message Format

A commitment signs the tuple `(salt, nonce, state)`. The `nonce` ensures
replay‑protection and must be unique per commitment. The functions return the
signature and echoed nonce:

```rust
use the_block::commit_reveal::{commit, verify};
let salt = b"channel";
let payload = b"data";
let (sig, nonce) = commit(salt, payload, 1);
assert!(verify(salt, payload, &sig, nonce));
```

## Dilithium Path (`pq-crypto` feature)

- Uses `dilithium3` to sign the message; signatures are truncated to
  170 bytes to simulate future compression.
- Nonces are 64‑bit little‑endian integers; verifying recomputes the message and
  checks the detached signature with the static keypair.
- Suitable when quantum resistance is required at the expense of larger
  signatures and slower verification.

## Hash Path (default)

Without `pq-crypto`, commitments are the BLAKE3 hash of the concatenated tuple.
Verification recomputes the hash and compares it to the provided signature.
This path is compact and fast but only offers classical security.

## Replay Protection

Callers must manage nonce lifetimes. A common pattern is to store the last used
nonce per `(salt, account)` pair and reject duplicates. Because the nonce is
part of the signed payload, reuse results in different signatures and fails
verification.

## Integration Points

- Mempool DoS prevention: nodes can require commitments for large payloads and
  only process reveals after a delay.
- Governance voting: proposals may commit hash roots of code before revealing
  full blobs once voting closes.

## Telemetry

`commit_reveal` does not emit metrics directly, but callers are expected to
record commit/reveal counts and verification failures via the central telemetry
crate.

See `node/src/commit_reveal.rs` for the implementation and
`node/tests/commit_reveal.rs` for round‑trip tests.