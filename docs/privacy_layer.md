# Privacy Layer

This prototype crate provides a minimal shielded transaction framework.
Notes commit to a value and random seed using BLAKE3. Nullifiers are
derived from note commitments to prevent double-spends. The `privacy`
crate is gated behind the `privacy` feature flag to reduce the default
attack surface.
