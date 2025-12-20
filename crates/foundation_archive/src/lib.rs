#![allow(
    clippy::manual_repeat_n,
    clippy::needless_range_loop,
    clippy::should_implement_trait
)]
#![forbid(unsafe_code)]

//! Archive utilities built in-house to remove third-party compression and
//! packaging crates.  The current implementation provides minimal TAR and Gzip
//! support tailored to the workspace use-cases while remaining fully
//! deterministic.  The modules intentionally expose small, focused APIs so the
//! underlying formats can evolve without cascading refactors.

pub mod gzip;
pub mod tar;
