#![forbid(unsafe_code)]

use crate::{Error, Result};

pub const OUTPUT_SIZE: usize = 20;

/// Compute the RIPEMD-160 digest of the provided data.
pub fn hash(_data: &[u8]) -> Result<[u8; OUTPUT_SIZE]> {
    Err(Error::unimplemented("hashing::ripemd160"))
}
