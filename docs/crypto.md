# Security & Cryptography
> **Review (2025-10-01):** Added coverage of the in-house Groth16 backend replacing prior bellman_ce usage.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

## Crypto Suite Overview

The `crates/crypto_suite` crate centralises our cryptographic primitives so the
node, CLI, wallet, and tooling all consume a single, audited implementation. It
is re-exported from `crypto/src/lib.rs`, which means existing `use crypto::…`
paths continue to compile while downstream crates migrate module-by-module.

The suite exposes the following modules:

- `signatures` — project-specific Ed25519 wrappers with PKCS#8 helpers.
- `transactions` — canonical bincode serialisation, domain-tag management, and
  signing/verifying utilities shared across binaries.
- `hashing` — First-party BLAKE3-compatible tree hash plus an in-house SHA3-256
  fallback.
- `key_derivation` — HKDF-based key derivation helpers used by wallet/session
  code.
- `zk` — Groth16 conveniences powered by the first-party BN254 R1CS engine.

Feature flags keep optional algorithms modular:

- `sha3-fallback` toggles the SHA3 adapters for environments that disallow
  BLAKE3.
- `dilithium` reserves room for the post-quantum signing backend.
- `threshold` is the placeholder gate for threshold-scheme support.

## Supported Algorithms

### Signatures & Transaction Domains

Ed25519 signing now flows through `crypto_suite::signatures::ed25519`, which
wraps the new first-party arithmetic implemented under
`signatures::ed25519_inhouse::{field, point, scalar}`. Keys expand via the
vendored SHA-512 helper, clamp into canonical scalars, and multiply against the
curve base point without relying on `ed25519-dalek`. The `transactions` module
owns the 16-byte domain tag (`TRANSACTION_DOMAIN_PREFIX`) and provides a
`TransactionSigner` that prepends the chain-specific domain before signing or
verifying.

Compatibility tests under `crates/crypto_suite/tests/transactions.rs` guarantee
that suite signatures follow the documented deterministic construction and that
domain tags reject cross-chain replays. `crates/crypto_suite/tests/inhouse_crypto.rs`
adds RFC8032 known-answer vectors, PKCS#8 round-trips, and strict failure cases
for non-canonical scalars, small-order points, and malformed encodings. The
same test module now exercises Groth16 verification against the in-house
constraint solver to ensure recorded circuits reject tampered inputs.

### Hashing & Commitments

The default hash (`crypto_suite::hashing::blake3`) implements a fully
tree-parallel BLAKE3-compatible construction with streaming, keyed hashing, and
derive-key helpers. All call sites previously importing the third-party crate
now route through the suite so remote signer payloads, ledger migrations, the
DEX, storage repair, and trie commitments share the same audited implementation.
An optional SHA3-256 fallback lives under `crypto_suite::hashing::sha3` for
deployments requiring the NIST sponge; it exposes identical APIs and shares the
same RFC 6234 vectors. Commitment proofs still record the algorithm so clients
can validate Merkle branches off-chain. Legacy RIPEMD-160, SHA-1 compatibility,
and CRC32 helpers now surface through `hashing::ripemd160`,
`hashing::sha1`, and `hashing::crc32` stubs that deliberately return
`Err(Unimplemented)` until the in-house backends land. Downstream crates should
plumb those errors to operators so the dependency freeze is visible during the
transition.

### Key Derivation

Wallet utilities and session keys use the shared HKDF-SHA256 helper provided by
`crypto_suite::key_derivation`. The function performs constant-time comparisons
to avoid timing leaks.

### Zero-Knowledge Proofs

`crypto_suite::zk::groth16` now fronts the first-party Groth16 implementation
under `zk::groth16_inhouse`. The suite retains the familiar parameter/proof
wrappers and legacy RNG shim while delegating constraint evaluation to the
BN254 engine shipped in-tree. Tests assert deterministic witness generation and
ensure verification fails when public inputs are tampered with.

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

This includes RFC8032 vectors, negative signature checks, signature
determinism/domain separation, and Groth16 witness/verification checks.

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
