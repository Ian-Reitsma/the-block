use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

/// Length in bytes of the ChaCha20-Poly1305 nonce stored alongside each chunk.
pub const CHACHA20_POLY1305_NONCE_LEN: usize = 12;
/// Authentication tag size emitted by ChaCha20-Poly1305.
pub const CHACHA20_POLY1305_TAG_LEN: usize = 16;
/// Total overhead added to each encrypted chunk (nonce + tag).
pub const ENCRYPTED_CHUNK_OVERHEAD: usize = CHACHA20_POLY1305_NONCE_LEN + CHACHA20_POLY1305_TAG_LEN;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Redundancy {
    None,
    ReedSolomon { data: u8, parity: u8 },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChunkRef {
    pub id: [u8; 32],
    pub nodes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ObjectManifest {
    pub version: u16,
    pub total_len: u64,
    pub chunk_len: u32,
    pub chunks: Vec<ChunkRef>,
    pub redundancy: Redundancy,
    pub content_key_enc: Vec<u8>,
    pub blake3: [u8; 32],
}

impl ObjectManifest {
    /// Number of plaintext chunks represented by the manifest.
    pub fn chunk_count(&self) -> usize {
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
        self.chunk_plain_len(index) + ENCRYPTED_CHUNK_OVERHEAD
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StoreReceipt {
    pub manifest_hash: [u8; 32],
    pub chunk_count: u32,
    pub redundancy: Redundancy,
    pub lane: String,
}
