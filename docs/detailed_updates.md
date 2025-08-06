# Detailed Update Log

This document captures the implementation notes from the recent token-amount and coinbase refactor.

## Overview

The chain now stores explicit coinbase values in each `Block`, wraps all amounts in the `TokenAmount` newtype, and migrates on-disk data to schema v3. Python bindings were updated to `pyo3` 0.24.2.

## Highlights

- **TokenAmount Wrapper** – Consensus-critical amounts are now wrapped in a transparent struct. Arithmetic is via helper methods only to ease a future move to `u128`.
- **Header Coinbase Fields** – `Block` records `coinbase_consumer` and `coinbase_industrial` for light-client validation. These are hashed in little-endian order.
- **Schema v3 Migration** – Opening the database upgrades older layouts and removes the legacy `accounts` and `emission` column families.
- **Hash Preimage Update** – The PoW preimage now includes the new coinbase fields. Genesis is regenerated accordingly.
- **Validation Order** – Reward checks occur only after proof-of-work is validated to avoid trivial DoS vectors.
- **Tests** – Added `test_coinbase_reward_recorded` and `test_import_reward_mismatch` plus updated schema gate assertions.
- **Python API** – Module definition uses `Bound<PyModule>` in accordance with `pyo3` 0.24.2.
- **TokenAmount Display** – Added `__repr__`, `__str__`, and `Display` trait implementations
  so amounts print as plain integers in both Python and Rust logs.
- **Python API Errors** – `fee_decompose` now raises distinct `ErrFeeOverflow` and `ErrInvalidSelector` exceptions for precise error handling.
- **Documentation** – Project disclaimers moved to README and Agents-Sup now details schema migrations and invariant anchors.
- **Test Harness Isolation** – `Blockchain::new` now provisions a unique temp directory per
  instance and removes it on drop. Fixtures call `unique_path` so parallel tests cannot
  interfere.
- **Replay Guard Test** – Reactivated `test_replay_attack_prevention` to prove duplicates
  with the same `(sender, nonce)` are rejected.

For the full rationale see `analysis.txt` and the commit history.

---

See [README.md#disclaimer](../README.md#disclaimer) for licensing and risk notices.
