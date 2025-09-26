pub mod blake3;
pub mod sha3;

use core::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct HashOutput {
    bytes: [u8; 32],
}

impl HashOutput {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    pub fn to_bytes(self) -> [u8; 32] {
        self.bytes
    }

    pub fn to_vec(self) -> Vec<u8> {
        self.bytes.to_vec()
    }

    pub fn to_hex(&self) -> HashHex {
        HashHex { bytes: self.bytes }
    }
}

impl Default for HashOutput {
    fn default() -> Self {
        Self { bytes: [0u8; 32] }
    }
}

impl From<[u8; 32]> for HashOutput {
    fn from(bytes: [u8; 32]) -> Self {
        Self::new(bytes)
    }
}

impl From<HashOutput> for [u8; 32] {
    fn from(value: HashOutput) -> Self {
        value.bytes
    }
}

impl fmt::Debug for HashOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("HashOutput")
            .field(&hex::encode(self.bytes))
            .finish()
    }
}

pub struct HashHex {
    bytes: [u8; 32],
}

impl HashHex {
    pub fn to_string(&self) -> String {
        hex::encode(self.bytes)
    }
}

impl fmt::Display for HashHex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.bytes))
    }
}

impl fmt::Debug for HashHex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.bytes))
    }
}

pub trait HashEngine {
    fn update(&mut self, data: &[u8]);
    fn finalize(self) -> HashOutput;
}

#[cfg(not(feature = "sha3-fallback"))]
pub use blake3::{hash as default_hash, Hasher as DefaultHasher};

#[cfg(feature = "sha3-fallback")]
pub use sha3::{hash as default_hash, Hasher as DefaultHasher};
