#![forbid(unsafe_code)]

use crate::{Error, Result};

/// Compute the CRC32 checksum for the supplied data slice.
pub fn checksum(_data: &[u8]) -> Result<u32> {
    Err(Error::unimplemented("hashing::crc32"))
}
