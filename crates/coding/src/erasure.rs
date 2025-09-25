use reed_solomon_erasure::galois_8::ReedSolomon;

const ALG_REED_SOLOMON: &str = "reed-solomon";
const ALG_XOR: &str = "xor";

use crate::error::{CodingError, ErasureError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErasureShardKind {
    Data,
    Parity,
}

#[derive(Clone, Debug)]
pub struct ErasureShard {
    pub index: usize,
    pub kind: ErasureShardKind,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct ErasureMetadata {
    pub data_shards: usize,
    pub parity_shards: usize,
    pub shard_len: usize,
    pub original_len: usize,
}

#[derive(Clone, Debug)]
pub struct ErasureBatch {
    pub metadata: ErasureMetadata,
    pub shards: Vec<ErasureShard>,
}

pub trait ErasureCoder: Send + Sync {
    fn algorithm(&self) -> &'static str;
    fn encode(&self, data: &[u8]) -> Result<ErasureBatch, ErasureError>;
    fn reconstruct(
        &self,
        metadata: &ErasureMetadata,
        shards: &[Option<ErasureShard>],
    ) -> Result<Vec<u8>, ErasureError>;
}

pub fn erasure_coder_for(
    name: &str,
    data_shards: usize,
    parity_shards: usize,
) -> Result<Box<dyn ErasureCoder>, CodingError> {
    match name {
        "" | ALG_REED_SOLOMON | "reed_solomon" | "rs" => Ok(Box::new(
            ReedSolomonErasureCoder::new(data_shards, parity_shards)?,
        )),
        ALG_XOR | "xor_parity" | "xor-parity" => {
            Ok(Box::new(XorCoder::new(data_shards, parity_shards)?))
        }
        other => Err(CodingError::UnsupportedAlgorithm {
            algorithm: other.to_string(),
        }),
    }
}

pub fn default_erasure_coder(
    data_shards: usize,
    parity_shards: usize,
) -> Result<Box<dyn ErasureCoder>, CodingError> {
    erasure_coder_for(ALG_REED_SOLOMON, data_shards, parity_shards)
}

pub struct ReedSolomonErasureCoder {
    data: usize,
    parity: usize,
    rs: ReedSolomon,
}

impl ReedSolomonErasureCoder {
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self, ErasureError> {
        if data_shards == 0 {
            return Err(ErasureError::InvalidShardCount {
                expected: 1,
                actual: 0,
            });
        }
        let rs = ReedSolomon::new(data_shards, parity_shards)
            .map_err(|e| ErasureError::EncodingFailed(e.to_string()))?;
        Ok(Self {
            data: data_shards,
            parity: parity_shards,
            rs,
        })
    }

    fn total(&self) -> usize {
        self.data + self.parity
    }
}

impl ErasureCoder for ReedSolomonErasureCoder {
    fn algorithm(&self) -> &'static str {
        ALG_REED_SOLOMON
    }

    fn encode(&self, data: &[u8]) -> Result<ErasureBatch, ErasureError> {
        let shard_len = if data.is_empty() {
            0
        } else {
            (data.len() + self.data - 1) / self.data
        };
        let total = self.total();
        let mut shards: Vec<Vec<u8>> = (0..total)
            .map(|idx| {
                if idx < self.data {
                    let start = idx * shard_len;
                    let end = usize::min(start + shard_len, data.len());
                    let mut shard = vec![0u8; shard_len];
                    if start < end {
                        shard[..end - start].copy_from_slice(&data[start..end]);
                    }
                    shard
                } else {
                    vec![0u8; shard_len]
                }
            })
            .collect();
        if shard_len > 0 {
            self.rs
                .encode(&mut shards)
                .map_err(|e| ErasureError::EncodingFailed(e.to_string()))?;
        }
        let metadata = ErasureMetadata {
            data_shards: self.data,
            parity_shards: self.parity,
            shard_len,
            original_len: data.len(),
        };
        let mut out = Vec::with_capacity(total);
        for (idx, shard) in shards.into_iter().enumerate() {
            let kind = if idx < self.data {
                ErasureShardKind::Data
            } else {
                ErasureShardKind::Parity
            };
            out.push(ErasureShard {
                index: idx,
                kind,
                bytes: shard,
            });
        }
        Ok(ErasureBatch {
            metadata,
            shards: out,
        })
    }

    fn reconstruct(
        &self,
        metadata: &ErasureMetadata,
        shards: &[Option<ErasureShard>],
    ) -> Result<Vec<u8>, ErasureError> {
        let total = metadata.data_shards + metadata.parity_shards;
        if shards.len() != total {
            return Err(ErasureError::InvalidShardCount {
                expected: total,
                actual: shards.len(),
            });
        }
        let mut buffers: Vec<Option<Vec<u8>>> = vec![None; total];
        for entry in shards.iter().flatten() {
            if entry.index >= total {
                return Err(ErasureError::InvalidShardIndex {
                    index: entry.index,
                    total,
                });
            }
            buffers[entry.index] = Some(entry.bytes.clone());
        }
        self.rs
            .reconstruct(&mut buffers)
            .map_err(|e| ErasureError::ReconstructionFailed(e.to_string()))?;
        let mut recovered = Vec::with_capacity(metadata.original_len);
        for shard in buffers.into_iter().take(metadata.data_shards) {
            let shard = shard.unwrap_or_default();
            recovered.extend_from_slice(&shard);
        }
        recovered.truncate(metadata.original_len);
        Ok(recovered)
    }
}

pub struct XorCoder {
    data: usize,
    parity: usize,
}

impl XorCoder {
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self, ErasureError> {
        if data_shards == 0 {
            return Err(ErasureError::InvalidShardCount {
                expected: 1,
                actual: 0,
            });
        }
        Ok(Self {
            data: data_shards,
            parity: parity_shards,
        })
    }

    fn total(&self) -> usize {
        self.data + self.parity
    }

    fn shard_len(&self, data_len: usize) -> usize {
        if data_len == 0 {
            0
        } else {
            (data_len + self.data - 1) / self.data
        }
    }

    fn build_data_shards(&self, data: &[u8], shard_len: usize) -> Vec<Vec<u8>> {
        (0..self.data)
            .map(|idx| {
                if shard_len == 0 {
                    return Vec::new();
                }
                let start = idx * shard_len;
                let end = usize::min(start + shard_len, data.len());
                let mut shard = vec![0u8; shard_len];
                if start < end {
                    shard[..end - start].copy_from_slice(&data[start..end]);
                }
                shard
            })
            .collect()
    }

    fn parity_template(&self, shard_len: usize) -> Vec<u8> {
        if shard_len == 0 {
            Vec::new()
        } else {
            vec![0u8; shard_len]
        }
    }

    fn xor_accumulate(target: &mut [u8], shard: &[u8]) {
        for (dst, src) in target.iter_mut().zip(shard.iter().copied()) {
            *dst ^= src;
        }
    }
}

impl ErasureCoder for XorCoder {
    fn algorithm(&self) -> &'static str {
        ALG_XOR
    }

    fn encode(&self, data: &[u8]) -> Result<ErasureBatch, ErasureError> {
        let shard_len = self.shard_len(data.len());
        let mut shards = self.build_data_shards(data, shard_len);
        if self.parity > 0 {
            let mut parity = self.parity_template(shard_len);
            for shard in &shards {
                Self::xor_accumulate(&mut parity, shard);
            }
            for _ in 0..self.parity {
                shards.push(parity.clone());
            }
        }
        let metadata = ErasureMetadata {
            data_shards: self.data,
            parity_shards: self.parity,
            shard_len,
            original_len: data.len(),
        };
        let mut out = Vec::with_capacity(self.total());
        for (idx, shard) in shards.into_iter().enumerate() {
            let kind = if idx < self.data {
                ErasureShardKind::Data
            } else {
                ErasureShardKind::Parity
            };
            out.push(ErasureShard {
                index: idx,
                kind,
                bytes: shard,
            });
        }
        Ok(ErasureBatch {
            metadata,
            shards: out,
        })
    }

    fn reconstruct(
        &self,
        metadata: &ErasureMetadata,
        shards: &[Option<ErasureShard>],
    ) -> Result<Vec<u8>, ErasureError> {
        let total = self.total();
        if shards.len() != total {
            return Err(ErasureError::InvalidShardCount {
                expected: total,
                actual: shards.len(),
            });
        }
        let mut buffers: Vec<Option<Vec<u8>>> = vec![None; total];
        for entry in shards.iter().flatten() {
            if entry.index >= total {
                return Err(ErasureError::InvalidShardIndex {
                    index: entry.index,
                    total,
                });
            }
            buffers[entry.index] = Some(entry.bytes.clone());
        }

        let mut missing_data = Vec::new();
        for idx in 0..self.data {
            if buffers[idx].is_none() {
                missing_data.push(idx);
            }
        }

        if missing_data.is_empty() {
            return finalize_recovered(metadata, buffers, self.data);
        }

        if self.parity == 0 {
            return Err(ErasureError::ReconstructionFailed(
                "no parity shards available for xor coder".to_string(),
            ));
        }
        if missing_data.len() > 1 {
            return Err(ErasureError::ReconstructionFailed(format!(
                "cannot recover {} missing data shards with xor parity",
                missing_data.len()
            )));
        }

        let mut parity = None;
        for idx in self.data..total {
            if let Some(ref shard) = buffers[idx] {
                parity = Some(shard.clone());
                break;
            }
        }

        let parity = parity.ok_or_else(|| {
            ErasureError::ReconstructionFailed("missing parity shard for xor recovery".to_string())
        })?;

        let shard_len = metadata.shard_len;
        if shard_len != parity.len() {
            return Err(ErasureError::ReconstructionFailed(
                "parity shard length mismatch".to_string(),
            ));
        }

        let mut recovered = parity;
        for (idx, maybe_shard) in buffers.iter().take(self.data).enumerate() {
            if missing_data.contains(&idx) {
                continue;
            }
            if let Some(ref shard) = maybe_shard {
                Self::xor_accumulate(&mut recovered, shard);
            } else {
                return Err(ErasureError::ReconstructionFailed(
                    "parity insufficient for xor recovery".to_string(),
                ));
            }
        }

        let missing_idx = missing_data[0];
        buffers[missing_idx] = Some(recovered);
        finalize_recovered(metadata, buffers, self.data)
    }
}

fn finalize_recovered(
    metadata: &ErasureMetadata,
    buffers: Vec<Option<Vec<u8>>>,
    data_shards: usize,
) -> Result<Vec<u8>, ErasureError> {
    let mut recovered = Vec::with_capacity(metadata.original_len);
    for shard in buffers.into_iter().take(data_shards) {
        let shard = shard.unwrap_or_default();
        recovered.extend_from_slice(&shard);
    }
    recovered.truncate(metadata.original_len);
    Ok(recovered)
}

pub fn canonical_algorithm_label(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return ALG_REED_SOLOMON.to_string();
    }
    let normalized = trimmed.replace('_', "-").to_ascii_lowercase();
    match normalized.as_str() {
        "rs" => ALG_REED_SOLOMON.to_string(),
        "reed-solomon" => ALG_REED_SOLOMON.to_string(),
        "xor" => ALG_XOR.to_string(),
        "xor-parity" => ALG_XOR.to_string(),
        other => other.to_string(),
    }
}
