use sha2::{Digest, Sha512 as Sha2Sha512};

pub struct Sha512;

impl Sha512 {
    pub fn digest(input: &[u8]) -> [u8; 64] {
        Self::digest_chunks(&[input])
    }

    pub fn digest_chunks(chunks: &[&[u8]]) -> [u8; 64] {
        let mut hasher = Sha2Sha512::new();
        for chunk in chunks {
            hasher.update(chunk);
        }
        let result = hasher.finalize();
        let mut out = [0u8; 64];
        out.copy_from_slice(&result);
        out
    }
}
