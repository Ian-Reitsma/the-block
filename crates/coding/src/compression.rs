use std::io::Cursor;

use crate::error::{CodingError, CompressionError};

pub trait Compressor: Send + Sync {
    fn algorithm(&self) -> &'static str;
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError>;
    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError>;
}

pub fn compressor_for(name: &str, level: i32) -> Result<Box<dyn Compressor>, CodingError> {
    match name {
        "" | "zstd" => Ok(Box::new(ZstdCompressor::new(level))),
        "noop" | "identity" => Ok(Box::new(NoopCompressor::default())),
        "rle" | "run_length" | "run-length" => Ok(Box::new(RleCompressor::default())),
        other => Err(CodingError::UnsupportedAlgorithm {
            algorithm: other.to_string(),
        }),
    }
}

pub fn default_compressor() -> Box<dyn Compressor> {
    Box::new(ZstdCompressor::new(0))
}

#[derive(Clone, Debug)]
pub struct ZstdCompressor {
    level: i32,
}

impl ZstdCompressor {
    pub fn new(level: i32) -> Self {
        Self { level }
    }
}

impl Compressor for ZstdCompressor {
    fn algorithm(&self) -> &'static str {
        "zstd"
    }

    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let cursor = Cursor::new(data);
        zstd::stream::encode_all(cursor, self.level)
            .map_err(|e| CompressionError::Compress(e.to_string()))
    }

    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let mut cursor = Cursor::new(data);
        zstd::stream::decode_all(&mut cursor)
            .map_err(|e| CompressionError::Decompress(e.to_string()))
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
