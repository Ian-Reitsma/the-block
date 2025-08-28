use super::hash_bytes;

/// Reference transcode workload: hash the input bytes.
pub fn run(data: &[u8]) -> [u8; 32] {
    hash_bytes(data)
}
