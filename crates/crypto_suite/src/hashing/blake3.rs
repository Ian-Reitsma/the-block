use blake3 as raw;

use super::{HashEngine, HashOutput};

pub type Hash = HashOutput;

#[derive(Clone)]
pub struct Hasher(raw::Hasher);

impl Hasher {
    pub fn new() -> Self {
        Self(raw::Hasher::new())
    }

    pub fn new_keyed(key: &[u8; 32]) -> Self {
        Self(raw::Hasher::new_keyed(key))
    }

    pub fn new_derive_key(context: &str) -> Self {
        Self(raw::Hasher::new_derive_key(context))
    }
}

impl HashEngine for Hasher {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn finalize(self) -> HashOutput {
        HashOutput::from(*self.0.finalize().as_bytes())
    }
}

pub fn hash(data: &[u8]) -> HashOutput {
    HashOutput::from(*raw::hash(data).as_bytes())
}

pub fn keyed_hash(key: &[u8; 32], data: &[u8]) -> HashOutput {
    HashOutput::from(*raw::keyed_hash(key, data).as_bytes())
}

pub fn derive_key(context: &str, material: &[u8]) -> [u8; 32] {
    raw::derive_key(context, material)
}
