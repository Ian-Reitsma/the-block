#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

pub mod executor;
pub mod future;
pub mod stream;
pub mod sync;
pub mod task;

pub use executor::block_on;
