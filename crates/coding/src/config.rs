use std::path::Path;

use foundation_serialization::toml;
use serde::{Deserialize, Serialize};

use crate::compression::{compressor_for, default_compressor};
use crate::encrypt::encryptor_for;
use crate::erasure::erasure_coder_for;
use crate::error::{CodingError, ConfigError};
use crate::fountain::fountain_coder_for;

pub const DEFAULT_FALLBACK_EMERGENCY_ENV: &str = "TB_STORAGE_FALLBACK_EMERGENCY";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptionConfig {
    #[serde(default = "default_encryptor_algorithm")]
    pub algorithm: String,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            algorithm: default_encryptor_algorithm(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErasureConfig {
    #[serde(default = "default_erasure_algorithm")]
    pub algorithm: String,
    #[serde(default = "default_data_shards")]
    pub data_shards: usize,
    #[serde(default = "default_parity_shards")]
    pub parity_shards: usize,
}

impl Default for ErasureConfig {
    fn default() -> Self {
        Self {
            algorithm: default_erasure_algorithm(),
            data_shards: default_data_shards(),
            parity_shards: default_parity_shards(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FountainConfig {
    #[serde(default = "default_fountain_algorithm")]
    pub algorithm: String,
    #[serde(default = "default_symbol_size")]
    pub symbol_size: u16,
    #[serde(default = "default_fountain_rate")]
    pub rate: f32,
}

impl Default for FountainConfig {
    fn default() -> Self {
        Self {
            algorithm: default_fountain_algorithm(),
            symbol_size: default_symbol_size(),
            rate: default_fountain_rate(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompressionConfig {
    #[serde(default = "default_compression_algorithm")]
    pub algorithm: String,
    #[serde(default = "default_compression_level")]
    pub level: i32,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: default_compression_algorithm(),
            level: default_compression_level(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkConfig {
    #[serde(default = "default_chunk_bytes")]
    pub default_bytes: usize,
    #[serde(default = "default_chunk_ladder")]
    pub ladder: Vec<usize>,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            default_bytes: default_chunk_bytes(),
            ladder: default_chunk_ladder(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub encryption: EncryptionConfig,
    #[serde(default)]
    pub erasure: ErasureConfig,
    #[serde(default)]
    pub fountain: FountainConfig,
    #[serde(default)]
    pub compression: CompressionConfig,
    #[serde(default)]
    pub chunks: ChunkConfig,
    #[serde(default)]
    pub rollout: RolloutConfig,
}

impl Config {
    pub fn encryptor(&self, key: &[u8]) -> Result<Box<dyn crate::Encryptor>, CodingError> {
        encryptor_for(&self.encryption.algorithm, key)
    }

    pub fn erasure_coder(&self) -> Result<Box<dyn crate::ErasureCoder>, CodingError> {
        erasure_coder_for(
            &self.erasure.algorithm,
            self.erasure.data_shards,
            self.erasure.parity_shards,
        )
    }

    pub fn fountain_coder(&self) -> Result<Box<dyn crate::FountainCoder>, CodingError> {
        fountain_coder_for(
            &self.fountain.algorithm,
            self.fountain.symbol_size,
            self.fountain.rate,
        )
    }

    pub fn compressor(&self) -> Result<Box<dyn crate::Compressor>, CodingError> {
        match compressor_for(&self.compression.algorithm, self.compression.level) {
            Ok(compressor) => Ok(compressor),
            Err(_) if self.compression.algorithm.is_empty() => Ok(default_compressor()),
            Err(err) => Err(err),
        }
    }

    pub fn chunk_default_bytes(&self) -> usize {
        self.chunks.default_bytes
    }

    pub fn chunk_ladder(&self) -> &[usize] {
        &self.chunks.ladder
    }

    pub fn rollout(&self) -> &RolloutConfig {
        &self.rollout
    }

    pub fn load_from_path(path: &Path) -> Result<Self, ConfigError> {
        let raw = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&raw)?)
    }
}

fn default_encryptor_algorithm() -> String {
    "chacha20-poly1305".to_string()
}

fn default_erasure_algorithm() -> String {
    "reed-solomon".to_string()
}

fn default_data_shards() -> usize {
    16
}

fn default_parity_shards() -> usize {
    8
}

fn default_fountain_algorithm() -> String {
    "lt-inhouse".to_string()
}

fn default_symbol_size() -> u16 {
    1024
}

fn default_fountain_rate() -> f32 {
    1.2
}

fn default_compression_algorithm() -> String {
    "lz77-rle".to_string()
}

fn default_compression_level() -> i32 {
    0
}

fn default_chunk_bytes() -> usize {
    1_048_576
}

fn default_chunk_ladder() -> Vec<usize> {
    vec![262_144, 524_288, 1_048_576, 2_097_152, 4_194_304]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RolloutConfig {
    #[serde(default)]
    pub allow_fallback_coder: bool,
    #[serde(default)]
    pub allow_fallback_compressor: bool,
    #[serde(default)]
    pub require_emergency_switch: bool,
    #[serde(default)]
    pub emergency_switch_env: Option<String>,
}

impl Default for RolloutConfig {
    fn default() -> Self {
        Self {
            allow_fallback_coder: false,
            allow_fallback_compressor: false,
            require_emergency_switch: false,
            emergency_switch_env: None,
        }
    }
}
