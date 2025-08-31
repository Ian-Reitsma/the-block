use super::hash_bytes;

/// Simulated GPU-accelerated hash workload.
pub fn run(data: &[u8]) -> [u8; 32] {
    // Placeholder for GPU offload; uses CPU hash for determinism.
    hash_bytes(data)
}
