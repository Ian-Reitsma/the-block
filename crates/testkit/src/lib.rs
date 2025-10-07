#![forbid(unsafe_code)]

//! Minimal first-party test harness scaffolding.
//!
//! The initial release only exposes helpers for parking tests while heavy
//! dependencies are being removed. Future iterations will provide property
//! testing, benchmarking, and fixture utilities implemented entirely in-house.

/// Marks a test as ignored while still compiling it, mirroring the previous
/// `#[ignore]` usage without depending on external macros.
#[macro_export]
macro_rules! ignored_test {
    ($name:ident, $body:block) => {
        #[test]
        #[ignore = "first-party test harness pending"]
        fn $name() $body
    };
}
