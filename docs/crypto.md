# Security & Cryptography

This document outlines recent cryptographic enhancements:

## Commitment Schemes

DEX escrow proofs now support both BLAKE3 and SHA3-256 commitment schemes.
Each escrow entry records the hash algorithm used so clients can verify proofs
off-chain.

## Key Derivation

Wallet utilities expose an HKDF-SHA256 based `derive_key` helper for generating
independent subkeys. The function performs constant-time comparisons to avoid
timing leaks.

## RPC Authentication

Administrative RPC tokens are compared in constant time so attackers cannot
leak secrets via timing side channels.

## Post-Quantum Signatures

The wallet crate optionally enables Dilithium2 signatures behind the
`dilithium` feature flag. This allows experimental post-quantum keypairs and
signatures without impacting default builds.

## Domain Separation

Signing operations use a 16-byte domain tag derived from the chain identifier.
`domain_tag_for` computes tags for alternate chains, and tests ensure that
mismatched tags reject signatures.

## Auditing

`cargo audit --deny warnings` runs in CI to ensure dependencies remain free of
known vulnerabilities.

