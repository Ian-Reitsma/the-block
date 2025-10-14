#![forbid(unsafe_code)]

//! Foundation serde facade that selects either the upstream implementation or
//! a first-party stub when `FIRST_PARTY_ONLY=1` or the `stub-backend` feature is
//! enabled.

#[cfg(all(feature = "external-backend", feature = "stub-backend"))]
compile_error!("foundation_serde backends are mutually exclusive; enable exactly one of `external-backend` or `stub-backend`.");

#[cfg(not(any(feature = "external-backend", feature = "stub-backend")))]
compile_error!(
    "foundation_serde requires a backend feature; enable `external-backend` or `stub-backend`."
);

#[cfg(feature = "external-backend")]
mod external {
    pub use serde::*;

    pub mod serde {
        pub use serde::*;
    }
}

#[cfg(feature = "external-backend")]
pub use external::*;

#[cfg(feature = "stub-backend")]
mod stub;
#[cfg(feature = "stub-backend")]
pub use stub::*;
