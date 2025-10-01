use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use super::{FountainBatch, FountainCoder, FountainError, FountainMetadata, FountainPacket};

const ALG_LT_INHOUSE: &str = "lt-inhouse";

pub struct InhouseLtFountain {
    symbol_size: u16,
    rate: f32,
}

impl InhouseLtFountain {
    pub fn new(symbol_size: u16, rate: f32) -> Result<Self, FountainError> {
        if symbol_size == 0 {
            return Err(FountainError::InvalidSymbolSize { size: symbol_size });
        }
        if !rate.is_finite() || rate < 1.0 {
            return Err(FountainError::InvalidRate { rate });
        }
        Ok(Self { symbol_size, rate })
    }

    fn symbol_size(&self) -> usize {
        self.symbol_size as usize
    }

    fn symbol_count(&self, len: usize) -> usize {
        if len == 0 {
            0
        } else {
            (len + self.symbol_size() - 1) / self.symbol_size()
        }
    }

    fn packet_budget(&self, symbols: usize) -> usize {
        if symbols == 0 {
            0
        } else {
            let overhead = (symbols as f32 * (self.rate - 1.0)).ceil() as usize;
            symbols + overhead
        }
    }

    fn build_symbols(&self, data: &[u8]) -> Vec<Vec<u8>> {
        let symbol_len = self.symbol_size();
        let count = self.symbol_count(data.len());
        let mut symbols = Vec::with_capacity(count);
        for idx in 0..count {
            let start = idx * symbol_len;
            let end = usize::min(start + symbol_len, data.len());
            let mut shard = vec![0u8; symbol_len];
            if start < end {
                shard[..end - start].copy_from_slice(&data[start..end]);
            }
            symbols.push(shard);
        }
        symbols
    }

    fn encode_indices(&self, symbol_count: usize, seq: usize) -> Vec<usize> {
        if symbol_count == 0 {
            return Vec::new();
        }
        if seq < symbol_count {
            return vec![seq];
        }
        let mut rng = StdRng::seed_from_u64(seq as u64);
        let degree = rng.gen_range(1..=symbol_count);
        let mut indices: Vec<usize> = (0..symbol_count).collect();
        indices.shuffle(&mut rng);
        indices.truncate(degree);
        indices.sort_unstable();
        indices
    }

    fn combine(symbols: &[Vec<u8>], indices: &[usize]) -> Vec<u8> {
        if indices.is_empty() {
            return vec![];
        }
        let len = symbols[0].len();
        let mut out = vec![0u8; len];
        for &idx in indices {
            for (dst, src) in out.iter_mut().zip(symbols[idx].iter()) {
                *dst ^= *src;
            }
        }
        out
    }

    fn parse_packet<'a>(
        &'a self,
        metadata: &FountainMetadata,
        packet: &'a FountainPacket,
    ) -> Result<(Vec<usize>, Vec<u8>), FountainError> {
        let bytes = packet.as_bytes();
        if bytes.len() < 8 {
            return Err(FountainError::PacketTruncated { len: bytes.len() });
        }
        let mut seed_bytes = [0u8; 8];
        seed_bytes.copy_from_slice(&bytes[..8]);
        let seed = u64::from_le_bytes(seed_bytes);
        let payload = bytes[8..].to_vec();
        let expected_len = metadata.symbol_size() as usize;
        if payload.len() != expected_len {
            return Err(FountainError::PacketTruncated { len: payload.len() });
        }
        let indices = self.decode_indices(metadata.symbol_count(), seed as usize);
        Ok((indices, payload))
    }

    fn decode_indices(&self, symbol_count: usize, seq: usize) -> Vec<usize> {
        if symbol_count == 0 {
            return Vec::new();
        }
        if seq < symbol_count {
            return vec![seq];
        }
        let mut rng = StdRng::seed_from_u64(seq as u64);
        let degree = rng.gen_range(1..=symbol_count);
        let mut indices: Vec<usize> = (0..symbol_count).collect();
        indices.shuffle(&mut rng);
        indices.truncate(degree);
        indices.sort_unstable();
        indices
    }
}

impl FountainCoder for InhouseLtFountain {
    fn algorithm(&self) -> &'static str {
        ALG_LT_INHOUSE
    }

    fn encode(&self, data: &[u8]) -> Result<FountainBatch, FountainError> {
        let symbol_count = self.symbol_count(data.len());
        let symbol_size = self.symbol_size();
        let symbols = self.build_symbols(data);
        let total_packets = self.packet_budget(symbol_count);
        let mut packets = Vec::with_capacity(total_packets);
        for seq in 0..total_packets {
            let indices = self.encode_indices(symbol_count, seq);
            let mut payload = if indices.is_empty() {
                vec![0u8; symbol_size]
            } else {
                Self::combine(&symbols, &indices)
            };
            payload.resize(symbol_size, 0);
            let mut bytes = Vec::with_capacity(8 + payload.len());
            bytes.extend_from_slice(&(seq as u64).to_le_bytes());
            bytes.extend_from_slice(&payload);
            packets.push(FountainPacket::new(bytes));
        }
        let metadata = FountainMetadata::new(self.symbol_size, data.len());
        Ok(FountainBatch { metadata, packets })
    }

    fn decode(
        &self,
        metadata: &FountainMetadata,
        packets: &[FountainPacket],
    ) -> Result<Vec<u8>, FountainError> {
        let symbol_count = metadata.symbol_count();
        if symbol_count == 0 {
            return Ok(Vec::new());
        }
        let mut equations = Vec::new();
        for packet in packets {
            let (indices, payload) = self.parse_packet(metadata, packet)?;
            if indices.is_empty() {
                continue;
            }
            equations.push((indices, payload));
        }
        let mut symbols: Vec<Option<Vec<u8>>> = vec![None; symbol_count];
        let symbol_len = metadata.symbol_size() as usize;
        let mut progress = true;
        while progress {
            progress = false;
            for (indices, payload) in equations.iter_mut() {
                if indices.is_empty() {
                    if payload.iter().any(|&b| b != 0) {
                        return Err(FountainError::Decode(
                            "inconsistent fountain packet set".to_string(),
                        ));
                    }
                    continue;
                }
                let mut remaining = Vec::with_capacity(indices.len());
                for &idx in indices.iter() {
                    if let Some(symbol) = &symbols[idx] {
                        for (dst, src) in payload.iter_mut().zip(symbol.iter()) {
                            *dst ^= *src;
                        }
                    } else {
                        remaining.push(idx);
                    }
                }
                *indices = remaining;
                if indices.len() == 1 {
                    let idx = indices[0];
                    if symbols[idx].is_none() {
                        let mut value = payload.clone();
                        value.truncate(symbol_len);
                        value.resize(symbol_len, 0);
                        symbols[idx] = Some(value);
                        progress = true;
                    }
                }
            }
        }
        if symbols.iter().any(|entry| entry.is_none()) {
            return Err(FountainError::InsufficientPackets);
        }
        let mut out = Vec::with_capacity(metadata.original_len());
        for symbol in symbols.into_iter().take(symbol_count) {
            out.extend_from_slice(symbol.unwrap().as_slice());
        }
        out.truncate(metadata.original_len());
        Ok(out)
    }
}
