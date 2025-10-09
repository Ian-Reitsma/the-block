use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{Deserialize, Serialize};

/// Header from an external chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub chain_id: String,
    pub height: u64,
    pub merkle_root: [u8; 32],
    pub signature: [u8; 32],
}

/// Merkle proof referencing a deposit leaf.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proof {
    pub leaf: [u8; 32],
    pub path: Vec<[u8; 32]>,
}

/// Hashes the header fields for signature comparison and replay protection.
pub fn header_hash(header: &Header) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(header.chain_id.as_bytes());
    h.update(&header.height.to_le_bytes());
    h.update(&header.merkle_root);
    *h.finalize().as_bytes()
}

/// Verifies the header signature and Merkle path.
pub fn verify(header: &Header, proof: &Proof) -> bool {
    if header_hash(header) != header.signature {
        return false;
    }
    let mut acc = proof.leaf;
    for sibling in &proof.path {
        let mut h = Hasher::new();
        h.update(&acc);
        h.update(sibling);
        acc = *h.finalize().as_bytes();
    }
    acc == header.merkle_root
}
