#![forbid(unsafe_code)]

use super::hash_bytes;

/// Execute a SNARK workload; for testing we simply hash the input bytes.
pub fn run(data: &[u8]) -> [u8; 32] {
    hash_bytes(data)
}
