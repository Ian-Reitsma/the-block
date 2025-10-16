use super::{FountainBatch, FountainCoder, FountainError, FountainMetadata, FountainPacket};
use crate::erasure::{
    ErasureCoder, ErasureMetadata, ErasureShard, ErasureShardKind, InhouseReedSolomon,
};

const HEADER_LEN: usize = 5;

const ALG_LT_INHOUSE: &str = "lt-inhouse";

pub struct InhouseLtFountain {
    _symbol_size: u16,
    _rate: f32,
}

impl InhouseLtFountain {
    pub fn new(symbol_size: u16, rate: f32) -> Result<Self, FountainError> {
        if symbol_size == 0 {
            return Err(FountainError::InvalidSymbolSize { size: symbol_size });
        }
        if !rate.is_finite() || rate < 1.0 {
            return Err(FountainError::InvalidRate { rate });
        }
        Ok(Self {
            _symbol_size: symbol_size,
            _rate: rate,
        })
    }

    fn parity_shards(&self, data_shards: usize) -> usize {
        if data_shards == 0 {
            return 0;
        }
        if self._rate <= 1.0 {
            return 0;
        }
        let extra = (self._rate - 1.0).max(0.0);
        let mut parity = (extra * data_shards as f32).ceil() as usize;
        if parity == 0 {
            parity = 1;
        }
        parity
    }

    fn encode_packet(shard: ErasureShard) -> FountainPacket {
        let mut bytes = Vec::with_capacity(HEADER_LEN + shard.bytes.len());
        let kind = match shard.kind {
            ErasureShardKind::Data => 0u8,
            ErasureShardKind::Parity => 1u8,
        };
        bytes.push(kind);
        bytes.extend_from_slice(&(shard.index as u32).to_le_bytes());
        bytes.extend_from_slice(&shard.bytes);
        FountainPacket::new(bytes)
    }

    fn decode_packet(
        packet: &FountainPacket,
        total: usize,
    ) -> Result<ErasureShard, FountainError> {
        let bytes = packet.as_bytes();
        if bytes.len() < HEADER_LEN {
            return Err(FountainError::Decode(
                "fountain packet truncated".to_string(),
            ));
        }
        let kind = match bytes[0] {
            0 => ErasureShardKind::Data,
            1 => ErasureShardKind::Parity,
            other => {
                return Err(FountainError::Decode(format!(
                    "unknown fountain packet kind {other}")))
            }
        };
        let index = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
        if index >= total {
            return Err(FountainError::Decode(format!(
                "fountain shard index {index} out of range {total}" )));
        }
        Ok(ErasureShard {
            index,
            kind,
            bytes: bytes[HEADER_LEN..].to_vec(),
        })
    }
}

impl FountainCoder for InhouseLtFountain {
    fn algorithm(&self) -> &'static str {
        ALG_LT_INHOUSE
    }

    fn encode(&self, data: &[u8]) -> Result<FountainBatch, FountainError> {
        let metadata = FountainMetadata::with_parity(
            self._symbol_size,
            data.len(),
            0,
        );
        if metadata.symbol_size() == 0 {
            return Err(FountainError::InvalidSymbolSize {
                size: self._symbol_size,
            });
        }
        let data_shards = metadata.symbol_count();
        if data_shards == 0 {
            return Ok(FountainBatch {
                metadata,
                packets: Vec::new(),
            });
        }
        let parity_shards = self.parity_shards(data_shards);
        let metadata = FountainMetadata::with_parity(
            self._symbol_size,
            data.len(),
            parity_shards,
        );
        let coder = InhouseReedSolomon::new(data_shards, parity_shards)
            .map_err(|err| FountainError::Encode(err.to_string()))?;
        let batch = coder
            .encode(data)
            .map_err(|err| FountainError::Encode(err.to_string()))?;
        let packets = batch
            .shards
            .into_iter()
            .map(Self::encode_packet)
            .collect();
        Ok(FountainBatch { metadata, packets })
    }

    fn decode(
        &self,
        metadata: &FountainMetadata,
        packets: &[FountainPacket],
    ) -> Result<Vec<u8>, FountainError> {
        let data_shards = metadata.symbol_count();
        if data_shards == 0 {
            return Ok(Vec::new());
        }
        let parity_shards = metadata.parity_count();
        let total = data_shards + parity_shards;
        if total == 0 {
            return Err(FountainError::Decode(
                "fountain metadata missing parity information".to_string(),
            ));
        }
        let coder = InhouseReedSolomon::new(data_shards, parity_shards)
            .map_err(|err| FountainError::Decode(err.to_string()))?;
        let mut slots: Vec<Option<ErasureShard>> = vec![None; total];
        for packet in packets {
            match Self::decode_packet(packet, total) {
                Ok(shard) => {
                    let index = shard.index;
                    slots[index] = Some(shard);
                }
                Err(err) => return Err(err),
            }
        }
        let erasure_meta = ErasureMetadata {
            data_shards,
            parity_shards,
            shard_len: metadata.symbol_size() as usize,
            original_len: metadata.original_len(),
        };
        coder
            .reconstruct(&erasure_meta, &slots)
            .map_err(|err| FountainError::Decode(err.to_string()))
    }
}
