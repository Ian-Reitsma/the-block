use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Redundancy {
    None,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChunkRef {
    pub id: [u8;32],
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
    pub blake3: [u8;32],
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StoreReceipt {
    pub manifest_hash: [u8;32],
    pub chunk_count: u32,
    pub redundancy: Redundancy,
    pub lane: String,
}
