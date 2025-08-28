use super::hash_bytes;

/// Reference inference workload: hash the input bytes.
pub fn run(data: &[u8]) -> [u8; 32] {
    hash_bytes(data)
}
