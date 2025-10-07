#![forbid(unsafe_code)]

use crate::{Error, Result};

pub const OUTPUT_SIZE: usize = 20;

/// Compute the SHA-1 digest of the provided data, mirroring the legacy API
/// until the first-party implementation lands.
pub fn hash(_data: &[u8]) -> Result<[u8; OUTPUT_SIZE]> {
    Err(Error::unimplemented("hashing::sha1"))
}
