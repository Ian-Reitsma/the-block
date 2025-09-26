# Schema Migration v9: DEX Escrow Tables
> **Review (2025-09-25):** Synced Schema Migration v9: DEX Escrow Tables guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Introduces an `escrow` table under the DEX state store to persist locked trades and
partial-payment proofs. Nodes upgrading from v8 initialise an empty escrow map and bump
`schema_version` to `9`.

No existing data is modified.
