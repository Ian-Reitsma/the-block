#![warn(missing_docs)]

//! In-house math primitives and numerical routines used across the codebase.
//! The goal is to replace external math dependencies with audited first-party
//! implementations tailored to the workload characteristics of The Block.

/// Probability distributions used across the codebase.
pub mod distribution;
/// Fixed-size linear algebra primitives.
pub mod linalg;
