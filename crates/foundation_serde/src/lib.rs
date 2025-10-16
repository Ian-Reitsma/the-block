#![forbid(unsafe_code)]

//! Foundation serde facade backed entirely by the in-house stub implementation.

mod stub;

pub use crate::stub::*;
