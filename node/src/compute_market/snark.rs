#![forbid(unsafe_code)]

/// Minimal Groth16-style proof helpers built on `bellman_ce`.
/// These routines do not implement a full circuit; instead they
/// derive a field element from the workload and output which is
/// then encoded as the "proof".  This keeps deterministic test
/// coverage while exercising the pairing-friendly field APIs that
/// a real Groth16/Plonk backend would rely upon.
use bellman_ce::pairing::bn256::Fr;
use bellman_ce::pairing::ff::PrimeField;
use blake3::Hasher;

/// Generate a deterministic pseudo-proof for a workload and output hash.
pub fn prove(wasm: &[u8], output: &[u8]) -> Vec<u8> {
    let mut h = Hasher::new();
    h.update(wasm);
    h.update(output);
    let digest = h.finalize();
    let mut wide = [0u8; 64];
    wide[..32].copy_from_slice(digest.as_bytes());
    Fr::from_bytes_wide(&wide).to_repr().as_ref().to_vec()
}

/// Verify the pseudo-proof by recomputing the field element.
pub fn verify(proof: &[u8], wasm: &[u8], output: &[u8]) -> bool {
    let mut h = Hasher::new();
    h.update(wasm);
    h.update(output);
    let digest = h.finalize();
    let mut wide = [0u8; 64];
    wide[..32].copy_from_slice(digest.as_bytes());
    let fr = Fr::from_bytes_wide(&wide);
    fr.to_repr().as_ref() == proof
}

/// Placeholder helper to compile WASM into a circuit representation.
/// Real implementation would leverage bellman/halo2 tooling.
pub fn compile_wasm(wasm: &[u8]) -> Vec<u8> {
    wasm.to_vec()
}
