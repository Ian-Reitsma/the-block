mod inhouse;

use crate::error::{CodingError, CompressionError};

pub trait Compressor: Send + Sync {
    fn algorithm(&self) -> &'static str;
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError>;
    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError>;
}

pub fn compressor_for(name: &str, level: i32) -> Result<Box<dyn Compressor>, CodingError> {
    match name {
        "" | "lz77" | "lz77-rle" | "hybrid" => Ok(Box::new(InhouseHybrid::new(level))),
        "noop" | "identity" => Ok(Box::new(NoopCompressor::default())),
        "rle" | "run_length" | "run-length" => Ok(Box::new(RleCompressor::default())),
        other => Err(CodingError::UnsupportedAlgorithm {
            algorithm: other.to_string(),
        }),
    }
}

pub fn default_compressor() -> Box<dyn Compressor> {
    Box::new(InhouseHybrid::new(4))
}

#[derive(Clone, Debug)]
pub struct InhouseHybrid {
    inner: inhouse::HybridCompressor,
}

impl InhouseHybrid {
    pub fn new(level: i32) -> Self {
        Self {
            inner: inhouse::HybridCompressor::new(level),
        }
    }
}

impl Compressor for InhouseHybrid {
    fn algorithm(&self) -> &'static str {
        "lz77-rle"
    }

    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        self.inner.compress(data)
    }

    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        self.inner.decompress(data)
    }
}

#[derive(Clone, Debug, Default)]
pub struct NoopCompressor;

impl Compressor for NoopCompressor {
    fn algorithm(&self) -> &'static str {
        "noop"
    }

    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        Ok(data.to_vec())
    }

    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        Ok(data.to_vec())
    }
}

#[derive(Clone, Debug, Default)]
pub struct RleCompressor;

impl Compressor for RleCompressor {
    fn algorithm(&self) -> &'static str {
        "rle"
    }

    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        if data.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(data.len());
        let mut iter = data.iter().copied();
        let mut current = iter.next().unwrap();
        let mut count: u8 = 1;
        for byte in iter {
            if byte == current && count < u8::MAX {
                count = count.saturating_add(1);
            } else {
                out.push(count);
                out.push(current);
                current = byte;
                count = 1;
            }
        }
        out.push(count);
        out.push(current);
        Ok(out)
    }

    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        if data.is_empty() {
            return Ok(Vec::new());
        }
        if data.len() % 2 != 0 {
            return Err(CompressionError::Decompress(
                "rle payload truncated".to_string(),
            ));
        }
        let mut out = Vec::new();
        for chunk in data.chunks_exact(2) {
            let count = chunk[0] as usize;
            let value = chunk[1];
            if count == 0 {
                return Err(CompressionError::Decompress(
                    "rle payload has zero-length run".to_string(),
                ));
            }
            out.extend(std::iter::repeat(value).take(count));
        }
        Ok(out)
    }
}
