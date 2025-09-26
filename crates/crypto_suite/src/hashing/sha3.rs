use sha3::{Digest, Sha3_256};

use super::{HashEngine, HashOutput};

#[derive(Clone)]
pub struct Hasher(Sha3_256);

impl Hasher {
    pub fn new() -> Self {
        Self(Sha3_256::new())
    }

    pub fn new_keyed(key: &[u8; 32]) -> Self {
        let mut hasher = Sha3_256::new();
        hasher.update(key);
        Self(hasher)
    }

    pub fn new_derive_key(context: &str) -> Self {
        let mut hasher = Sha3_256::new();
        hasher.update(context.as_bytes());
        Self(hasher)
    }
}

impl HashEngine for Hasher {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn finalize(self) -> HashOutput {
        let output = self.0.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&output);
        HashOutput::from(bytes)
    }
}

pub fn hash(data: &[u8]) -> HashOutput {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

pub fn keyed_hash(key: &[u8; 32], data: &[u8]) -> HashOutput {
    let mut hasher = Hasher::new_keyed(key);
    hasher.update(data);
    hasher.finalize()
}

pub fn derive_key(context: &str, material: &[u8]) -> [u8; 32] {
    let mut hasher = Hasher::new_derive_key(context);
    hasher.update(material);
    hasher.finalize().to_bytes()
}
