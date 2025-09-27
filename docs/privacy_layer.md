# Privacy Layer
> **Review (2025-09-25):** Synced Privacy Layer guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

This prototype crate provides a minimal shielded transaction framework.
Notes commit to a value and random seed using BLAKE3. Nullifiers are
derived from note commitments to prevent double-spends. The `privacy`
crate is gated behind the `privacy` feature flag to reduce the default
attack surface.
