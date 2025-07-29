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

For the full rationale see `analysis.txt` and the commit history.
