#![cfg(feature = "integration-tests")]
//! Test-vectors helper module.
//!
//! This file intentionally contains no code; it exists so that `mod vectors;`
//! statements in the test tree resolve cleanly even when no helpers are
//! required.  Tests that rely on external vector data can either populate this
//! module with helpers or place fixtures under `tests/vectors/`.
//!
//! Keeping the stub prevents accidental compile failures when the `fuzzy`
//! feature is enabled during lint runs and serves as a reminder of where
//! vector-related utilities belong.
