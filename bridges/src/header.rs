use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowHeader {
    pub chain_id: String,
    pub height: u64,
    pub merkle_root: [u8; 32],
    /// BLAKE3(header fields)
    pub signature: [u8; 32],
    pub nonce: u64,
    pub target: u64,
}

pub fn hash_header(h: &PowHeader) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(h.chain_id.as_bytes());
    hasher.update(&h.height.to_le_bytes());
    hasher.update(&h.merkle_root);
    hasher.update(&h.signature);
    hasher.update(&h.nonce.to_le_bytes());
    *hasher.finalize().as_bytes()
}

pub fn verify_pow(h: &PowHeader) -> bool {
    let hash = hash_header(h);
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash[..8]);
    u64::from_le_bytes(bytes) < h.target
}
