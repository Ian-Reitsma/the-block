pub mod inhouse;

use crate::error::{CodingError, FountainError};

pub use self::inhouse::InhouseLtFountain;

#[derive(Clone, Debug)]
pub struct FountainPacket {
    bytes: Vec<u8>,
}

impl FountainPacket {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Clone, Debug)]
pub struct FountainMetadata {
    symbol_size: u16,
    symbol_count: usize,
    parity_count: usize,
    original_len: usize,
}

impl FountainMetadata {
    pub fn new(symbol_size: u16, original_len: usize) -> Self {
        Self::with_parity(symbol_size, original_len, 0)
    }

    pub fn with_parity(symbol_size: u16, original_len: usize, parity_count: usize) -> Self {
        let count = if symbol_size == 0 {
            0
        } else if original_len == 0 {
            0
        } else {
            (original_len + symbol_size as usize - 1) / symbol_size as usize
        };
        Self {
            symbol_size,
            symbol_count: count,
            parity_count,
            original_len,
        }
    }

    pub fn symbol_size(&self) -> u16 {
        self.symbol_size
    }

    pub fn symbol_count(&self) -> usize {
        self.symbol_count
    }

    pub fn parity_count(&self) -> usize {
        self.parity_count
    }

    pub fn original_len(&self) -> usize {
        self.original_len
    }
}

#[derive(Clone, Debug)]
pub struct FountainBatch {
    metadata: FountainMetadata,
    packets: Vec<FountainPacket>,
}

impl FountainBatch {
    pub fn metadata(&self) -> &FountainMetadata {
        &self.metadata
    }

    pub fn packets(&self) -> &[FountainPacket] {
        &self.packets
    }

    pub fn into_parts(self) -> (FountainMetadata, Vec<FountainPacket>) {
        (self.metadata, self.packets)
    }
}

pub trait FountainCoder: Send + Sync {
    fn algorithm(&self) -> &'static str;
    fn encode(&self, data: &[u8]) -> Result<FountainBatch, FountainError>;
    fn decode(
        &self,
        metadata: &FountainMetadata,
        packets: &[FountainPacket],
    ) -> Result<Vec<u8>, FountainError>;
}

pub fn fountain_coder_for(
    name: &str,
    symbol_size: u16,
    rate: f32,
) -> Result<Box<dyn FountainCoder>, CodingError> {
    match name.trim().to_ascii_lowercase().as_str() {
        "" | "lt-inhouse" | "lt" | "fountain" => {
            Ok(Box::new(InhouseLtFountain::new(symbol_size, rate)?))
        }
        other => Err(CodingError::UnsupportedAlgorithm {
            algorithm: other.to_string(),
        }),
    }
}

pub fn default_fountain_coder(
    symbol_size: u16,
    rate: f32,
) -> Result<Box<dyn FountainCoder>, CodingError> {
    fountain_coder_for("lt-inhouse", symbol_size, rate)
}
