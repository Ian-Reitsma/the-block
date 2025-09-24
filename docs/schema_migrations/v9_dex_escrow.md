# Schema Migration v9: DEX Escrow Tables
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

Introduces an `escrow` table under the DEX state store to persist locked trades and
partial-payment proofs. Nodes upgrading from v8 initialise an empty escrow map and bump
`schema_version` to `9`.

No existing data is modified.