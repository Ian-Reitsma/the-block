# Quantum-Safe Signature Migration
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

This note outlines a phased migration from the current Ed25519 scheme to a dual-key model incorporating Dilithium.

## Phases

1. **Dual-Key Introduction** – transactions include both Ed25519 and Dilithium signatures.  Nodes verify both signatures when the `quantum` feature flag is enabled.
2. **Macro Adoption** – block headers optionally carry a Dilithium miner signature, enabling chain-wide measurement of verification cost.
3. **Dilithium-Only Mode** – once confidence is established, accounts may opt into Dilithium-only transactions and miners may produce blocks signed solely with Dilithium.

## Replay Protection

Each signature scheme uses a distinct domain‑separation tag.  Legacy transactions continue to validate under Ed25519 rules, while dual‑key and Dilithium‑only transactions are versioned to prevent cross‑scheme replays.

## Migration Utility

A dedicated tool will derive Dilithium keypairs alongside existing Ed25519 keys and update account records on chain.  Historical signatures remain valid and traceable through the account's version history.
