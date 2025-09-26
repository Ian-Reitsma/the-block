# Security & Cryptography
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

## Crypto Suite Overview

The `crates/crypto_suite` crate centralises our cryptographic primitives so the
node, CLI, wallet, and tooling all consume a single, audited implementation. It
is re-exported from `crypto/src/lib.rs`, which means existing `use crypto::…`
paths continue to compile while downstream crates migrate module-by-module.

The suite exposes the following modules:

- `signatures` — project-specific Ed25519 wrappers with PKCS#8 helpers.
- `transactions` — canonical bincode serialisation, domain-tag management, and
  signing/verifying utilities shared across binaries.
- `hashing` — BLAKE3 by default with an optional SHA3 fallback.
- `key_derivation` — HKDF-based key derivation helpers used by wallet/session
  code.
- `zk` — Groth16 conveniences that wrap `bellman_ce` behind a stable API.

Feature flags keep optional algorithms modular:

- `sha3-fallback` toggles the SHA3 adapters for environments that disallow
  BLAKE3.
- `dilithium` reserves room for the post-quantum signing backend.
- `threshold` is the placeholder gate for threshold-scheme support.

## Supported Algorithms

### Signatures & Transaction Domains

Ed25519 signing now flows through `crypto_suite::signatures::ed25519`, which
wraps `ed25519-dalek` while hiding crate-specific types. The `transactions`
module owns the 16-byte domain tag (`TRANSACTION_DOMAIN_PREFIX`) and provides a
`TransactionSigner` that prepends the chain-specific domain before signing or
verifying.

Compatibility tests under `crates/crypto_suite/tests/transactions.rs` guarantee
that suite signatures match direct `ed25519-dalek` output and that domain tags
reject cross-chain replays. The same test module exercises Groth16 verification
against the raw `bellman_ce` API.

### Hashing & Commitments

BLAKE3 remains the primary hashing algorithm and now lives behind
`crypto_suite::hashing`. Enabling the `sha3-fallback` feature swaps in SHA3-256
for deployments where BLAKE3 is disallowed. Commitment schemes in the DEX stack
still record the hash algorithm used so clients can verify proofs off-chain.

### Key Derivation

Wallet utilities and session keys use the shared HKDF-SHA256 helper provided by
`crypto_suite::key_derivation`. The function performs constant-time comparisons
to avoid timing leaks.

### Zero-Knowledge Proofs

`crypto_suite::zk::groth16` wraps the `bellman_ce` Groth16 implementation. The
suite exposes parameter/proof wrappers, prepared verifying keys, and a legacy
RNG shim for compatibility with older `rand` versions. Tests assert parity with
direct `bellman_ce::groth16::verify_proof` calls.

### RPC Authentication

Administrative RPC tokens continue to rely on constant-time comparisons so
attackers cannot leak secrets via timing side channels.

### Post-Quantum Signatures

The wallet crate keeps Dilithium2 support behind the `dilithium` feature flag so
experiments do not impact default builds. The crypto suite’s `dilithium` flag
reserves the slot for a first-party backend.

## Testing & Benchmarks

Run the suite’s unit tests with:

```bash
cargo test -p crypto_suite
```

This includes signature compatibility, domain-separation, and Groth16 parity
checks. The `ed25519` Criterion benchmark compares suite signing performance
against direct `ed25519_dalek` calls:

```bash
cargo bench -p crypto_suite --bench ed25519
```

## Adding New Algorithms

1. Implement the primitive inside `crates/crypto_suite` using the existing
   module layout (e.g. `signatures::` or a new submodule).
2. Gate optional functionality behind a feature flag and document it here.
3. Expose trait-based APIs so downstream crates remain decoupled from concrete
   dependencies.
4. Add integration tests alongside `crates/crypto_suite/tests/transactions.rs`
   to assert compatibility with any legacy implementation.
5. Update benchmarking code when introducing performance-sensitive changes.

## Domain Separation

Signing operations use a 16-byte domain tag derived from the chain identifier.
`domain_tag_for` computes tags for alternate chains, and tests ensure that
mismatched tags reject signatures.

## Auditing

`cargo audit --deny warnings` runs in CI to ensure dependencies remain free of
known vulnerabilities.
