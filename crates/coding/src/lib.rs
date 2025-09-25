mod compression;
mod config;
mod encrypt;
mod erasure;
mod error;
mod fountain;

pub use compression::{
    compressor_for, default_compressor, Compressor, NoopCompressor, RleCompressor, ZstdCompressor,
};
pub use config::{
    ChunkConfig, CompressionConfig, Config, EncryptionConfig, ErasureConfig, FountainConfig,
    RolloutConfig, DEFAULT_FALLBACK_EMERGENCY_ENV,
};
pub use encrypt::{
    default_encryptor, encryptor_for, ChaCha20Poly1305Encryptor, Encryptor,
    CHACHA20_POLY1305_KEY_LEN, CHACHA20_POLY1305_NONCE_LEN, CHACHA20_POLY1305_TAG_LEN,
};
pub use erasure::{
    canonical_algorithm_label, default_erasure_coder, erasure_coder_for, ErasureBatch,
    ErasureCoder, ErasureMetadata, ErasureShard, ErasureShardKind, ReedSolomonErasureCoder,
    XorCoder,
};
pub use error::{
    CodingError, CompressionError, ConfigError, EncryptError, ErasureError, FountainError,
};
pub use fountain::{
    default_fountain_coder, fountain_coder_for, FountainBatch, FountainCoder, FountainMetadata,
    FountainPacket, RaptorqFountainCoder,
};

#[cfg(test)]
mod tests {
    use super::{Compressor, Config, ErasureCoder};

    #[test]
    fn encrypt_round_trip() {
        let cfg = Config::default();
        let key = [42u8; super::CHACHA20_POLY1305_KEY_LEN];
        let encryptor = cfg.encryptor(&key).expect("encryptor");
        let payload = b"storage round trip";
        let ciphertext = encryptor.encrypt(payload).expect("encrypt");
        let recovered = encryptor.decrypt(&ciphertext).expect("decrypt");
        assert_eq!(recovered, payload);
    }

    #[test]
    fn reed_solomon_recovers_missing_shards() {
        let cfg = Config::default();
        let coder = cfg.erasure_coder().expect("erasure coder");
        let data = vec![0x55u8; 4096];
        let batch = coder.encode(&data).expect("encode");
        let mut slots = vec![None; batch.shards.len()];
        for shard in batch.shards.iter().cloned() {
            let idx = shard.index;
            slots[idx] = Some(shard);
        }
        slots[0] = None;
        slots[3] = None;
        let recovered = coder
            .reconstruct(&batch.metadata, &slots)
            .expect("reconstruct");
        assert_eq!(recovered, data);
    }

    #[test]
    fn xor_coder_recovers_single_missing_shard() {
        let coder = super::XorCoder::new(4, 1).expect("xor coder");
        let data: Vec<u8> = (0..2048).map(|idx| (idx % 251) as u8).collect();
        let batch = coder.encode(&data).expect("encode");
        let mut slots: Vec<Option<super::ErasureShard>> = vec![None; batch.shards.len()];
        for shard in batch.shards.into_iter() {
            let idx = shard.index;
            slots[idx] = Some(shard);
        }
        slots[2] = None;
        let recovered = coder
            .reconstruct(&batch.metadata, &slots)
            .expect("reconstruct");
        assert_eq!(recovered, data);
    }

    #[test]
    fn zstd_compression_reduces_size() {
        let cfg = Config::default();
        let compressor = cfg.compressor().expect("compressor");
        let data = vec![b'a'; 16 * 1024];
        let compressed = compressor.compress(&data).expect("compress");
        assert!(compressed.len() < data.len());
        let decompressed = compressor.decompress(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn rle_compression_round_trip() {
        let compressor = super::RleCompressor::default();
        let mut data = Vec::new();
        data.extend(std::iter::repeat(b'z').take(128));
        data.extend((0u8..64).collect::<Vec<_>>());
        data.extend(std::iter::repeat(b'q').take(256));
        let compressed = compressor.compress(&data).expect("compress");
        assert!(compressed.len() <= data.len());
        let decompressed = compressor.decompress(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }
}
