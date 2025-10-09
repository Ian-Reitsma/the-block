use foundation_serialization::{Deserialize, Serialize};
use std::convert::TryFrom;

pub use coding::CHACHA20_POLY1305_NONCE_LEN;
pub use coding::CHACHA20_POLY1305_TAG_LEN;
/// Total overhead added to each encrypted chunk (nonce + tag).
pub const ENCRYPTED_CHUNK_OVERHEAD: usize =
    coding::CHACHA20_POLY1305_NONCE_LEN + coding::CHACHA20_POLY1305_TAG_LEN;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(crate = "foundation_serialization::serde")]
pub enum Redundancy {
    None,
    ReedSolomon { data: u8, parity: u8 },
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ProviderChunkEntry {
    pub provider: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub chunk_indices: Vec<u32>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub chunk_lens: Vec<u32>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub encryption_key: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ChunkRef {
    pub id: [u8; 32],
    pub nodes: Vec<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub provider_chunks: Vec<ProviderChunkEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ObjectManifest {
    pub version: u16,
    pub total_len: u64,
    pub chunk_len: u32,
    pub chunks: Vec<ChunkRef>,
    pub redundancy: Redundancy,
    pub content_key_enc: Vec<u8>,
    pub blake3: [u8; 32],
    #[serde(default = "foundation_serialization::defaults::default")]
    pub chunk_lens: Vec<u32>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub chunk_compressed_lens: Vec<u32>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub chunk_cipher_lens: Vec<u32>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub compression_alg: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub compression_level: Option<i32>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub encryption_alg: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub erasure_alg: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    pub provider_chunks: Vec<ProviderChunkEntry>,
}

impl ObjectManifest {
    /// Number of plaintext chunks represented by the manifest.
    pub fn chunk_count(&self) -> usize {
        if !self.chunk_lens.is_empty() {
            return self.chunk_lens.len();
        }
        let chunk_len = self.chunk_len as u64;
        if chunk_len == 0 {
            return 0;
        }
        if self.total_len == 0 {
            return if self.chunks.is_empty() { 0 } else { 1 };
        }
        ((self.total_len + chunk_len - 1) / chunk_len) as usize
    }

    /// Plaintext length of the `index`-th chunk prior to encryption.
    pub fn chunk_plain_len(&self, index: usize) -> usize {
        if !self.chunk_lens.is_empty() {
            return self.chunk_lens.get(index).copied().unwrap_or(0) as usize;
        }
        let chunk_len = self.chunk_len as usize;
        if chunk_len == 0 {
            return 0;
        }
        let total = usize::try_from(self.total_len).unwrap_or(usize::MAX);
        let start = chunk_len.saturating_mul(index);
        if start >= total {
            return 0;
        }
        (total - start).min(chunk_len)
    }

    /// Ciphertext length (nonce + ciphertext) stored for the `index`-th chunk.
    pub fn chunk_cipher_len(&self, index: usize) -> usize {
        if !self.chunk_cipher_lens.is_empty() {
            return self.chunk_cipher_lens.get(index).copied().unwrap_or(0) as usize;
        }
        self.chunk_plain_len(index) + ENCRYPTED_CHUNK_OVERHEAD
    }

    /// Compressed length stored for the `index`-th chunk prior to encryption.
    pub fn chunk_compressed_len(&self, index: usize) -> usize {
        if !self.chunk_compressed_lens.is_empty() {
            return self.chunk_compressed_lens.get(index).copied().unwrap_or(0) as usize;
        }
        self.chunk_plain_len(index)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "foundation_serialization::serde")]
pub struct StoreReceipt {
    pub manifest_hash: [u8; 32],
    pub chunk_count: u32,
    pub redundancy: Redundancy,
    pub lane: String,
}
