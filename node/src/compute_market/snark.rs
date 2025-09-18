#![forbid(unsafe_code)]

use blake3::Hasher;

/// Generate a deterministic pseudo-proof for a workload and output hash.
pub fn prove(wasm: &[u8], output: &[u8]) -> Vec<u8> {
    let mut h = Hasher::new();
    h.update(wasm);
    h.update(output);
    h.finalize().as_bytes().to_vec()
}

/// Verify the pseudo-proof by recomputing the field element.
pub fn verify(proof: &[u8], wasm: &[u8], output: &[u8]) -> bool {
    let mut h = Hasher::new();
    h.update(wasm);
    h.update(output);
    h.finalize().as_bytes() == proof
}

/// Placeholder helper to compile WASM into a circuit representation.
/// Real implementation would leverage bellman/halo2 tooling.
pub fn compile_wasm(wasm: &[u8]) -> Vec<u8> {
    wasm.to_vec()
}
