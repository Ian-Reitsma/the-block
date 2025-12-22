#![allow(clippy::excessive_precision, clippy::needless_range_loop)]
#![warn(missing_docs)]

//! In-house math primitives and numerical routines used across the codebase.
//! The goal is to replace external math dependencies with audited first-party
//! implementations tailored to the workload characteristics of The Block.

/// Probability distributions used across the codebase.
pub mod distribution;
/// Fixed-size linear algebra primitives.
pub mod linalg;
/// Integer helper traits mirroring the tiny slice of `num-traits` still used.
pub mod numbers;
/// Spectral transforms and related helpers.
pub mod transform;

pub use numbers::{Integer, One, Zero};

#[cfg(test)]
pub mod testing;
