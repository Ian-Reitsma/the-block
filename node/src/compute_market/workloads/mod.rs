pub mod gpu;
pub mod inference;
pub mod snark;
pub mod transcode;

use blake3::Hasher;

pub fn hash_bytes(data: &[u8]) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(data);
    *h.finalize().as_bytes()
}
