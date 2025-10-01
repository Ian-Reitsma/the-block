use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodingError {
    #[error("unsupported algorithm: {algorithm}")]
    UnsupportedAlgorithm { algorithm: String },
    #[error("algorithm disabled by rollout policy: {algorithm}")]
    Disabled { algorithm: String },
    #[error(transparent)]
    Encrypt(#[from] EncryptError),
    #[error(transparent)]
    Erasure(#[from] ErasureError),
    #[error(transparent)]
    Fountain(#[from] FountainError),
    #[error(transparent)]
    Compression(#[from] CompressionError),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("coding config io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("coding config parse failed: {0}")]
    Parse(#[from] toml::de::Error),
}

#[derive(Debug, Error)]
pub enum EncryptError {
    #[error("invalid key length: expected {expected} bytes, got {actual}")]
    InvalidKeyLength { expected: usize, actual: usize },
    #[error("ciphertext too short: {len} bytes")]
    InvalidCiphertext { len: usize },
    #[error("encryption failed")]
    EncryptionFailed,
    #[error("decryption failed")]
    DecryptionFailed,
}

#[derive(Debug, Error)]
pub enum ErasureError {
    #[error("invalid shard count: expected {expected}, got {actual}")]
    InvalidShardCount { expected: usize, actual: usize },
    #[error("invalid shard index {index} for total {total}")]
    InvalidShardIndex { index: usize, total: usize },
    #[error("insufficient shards: expected {expected}, available {available}")]
    InsufficientShards { expected: usize, available: usize },
    #[error("erasure encoding failed: {0}")]
    EncodingFailed(String),
    #[error("erasure reconstruction failed: {0}")]
    ReconstructionFailed(String),
}

#[derive(Debug, Error)]
pub enum FountainError {
    #[error("invalid fountain symbol size: {size}")]
    InvalidSymbolSize { size: u16 },
    #[error("invalid fountain rate: {rate}")]
    InvalidRate { rate: f32 },
    #[error("fountain encode failed: {0}")]
    Encode(String),
    #[error("fountain decode failed: {0}")]
    Decode(String),
    #[error("fountain packet truncated: {len} bytes")]
    PacketTruncated { len: usize },
    #[error("insufficient fountain packets to decode")]
    InsufficientPackets,
}

#[derive(Debug, Error)]
pub enum CompressionError {
    #[error("compression failed: {0}")]
    Compress(String),
    #[error("decompression failed: {0}")]
    Decompress(String),
}
