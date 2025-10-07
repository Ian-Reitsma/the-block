#![forbid(unsafe_code)]

use crate::Result;

/// IEEE CRC32 polynomial in reversed representation.
const POLY: u32 = 0xEDB8_8320;

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

const TABLE: [u32; 256] = build_table();

/// Compute the CRC32 checksum for the supplied data slice.
pub fn checksum(data: &[u8]) -> Result<u32> {
    let mut crc = !0u32;
    for &byte in data {
        let idx = ((crc as u8) ^ byte) as usize;
        crc = (crc >> 8) ^ TABLE[idx];
    }
    Ok(!crc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_known_vectors() {
        assert_eq!(checksum(b"").unwrap(), 0x0000_0000);
        assert_eq!(checksum(b"123456789").unwrap(), 0xCBF4_3926);
        assert_eq!(checksum(b"hello world").unwrap(), 0x0D4A_1185);
    }

    #[test]
    fn checksum_handles_long_inputs() {
        let data = vec![0xAA; 1_024];
        assert_eq!(checksum(&data).unwrap(), 0x3C6F_327D);
    }
}
