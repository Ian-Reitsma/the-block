use raptorq::{Decoder, Encoder, EncodingPacket, ObjectTransmissionInformation};

use crate::error::{CodingError, FountainError};

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
    inner: FountainMetadataInner,
}

#[derive(Clone, Debug)]
struct FountainMetadataInner {
    oti: ObjectTransmissionInformation,
    original_len: usize,
}

impl FountainMetadata {
    fn new(oti: ObjectTransmissionInformation, original_len: usize) -> Self {
        Self {
            inner: FountainMetadataInner { oti, original_len },
        }
    }

    pub fn original_len(&self) -> usize {
        self.inner.original_len
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
    match name {
        "" | "raptorq" | "raptor-q" => Ok(Box::new(RaptorqFountainCoder::new(symbol_size, rate))),
        other => Err(CodingError::UnsupportedAlgorithm {
            algorithm: other.to_string(),
        }),
    }
}

pub fn default_fountain_coder(
    symbol_size: u16,
    rate: f32,
) -> Result<Box<dyn FountainCoder>, CodingError> {
    fountain_coder_for("raptorq", symbol_size, rate)
}

pub struct RaptorqFountainCoder {
    symbol_size: u16,
    rate: f32,
}

impl RaptorqFountainCoder {
    pub fn new(symbol_size: u16, rate: f32) -> Self {
        Self { symbol_size, rate }
    }

    fn packet_count(&self, len: usize) -> u32 {
        let symbols = (len as f32 / f32::from(self.symbol_size)).ceil();
        let repair = ((symbols * (self.rate - 1.0)).ceil()).max(0.0) as u32;
        symbols as u32 + repair
    }
}

impl FountainCoder for RaptorqFountainCoder {
    fn algorithm(&self) -> &'static str {
        "raptorq"
    }

    fn encode(&self, data: &[u8]) -> Result<FountainBatch, FountainError> {
        let oti = ObjectTransmissionInformation::with_defaults(data.len() as u64, self.symbol_size);
        let encoder = Encoder::new(data, oti.clone());
        let total = self.packet_count(data.len());
        let packets = encoder
            .get_encoded_packets(total)
            .into_iter()
            .map(|packet| FountainPacket::new(packet.serialize()))
            .collect();
        Ok(FountainBatch {
            metadata: FountainMetadata::new(oti, data.len()),
            packets,
        })
    }

    fn decode(
        &self,
        metadata: &FountainMetadata,
        packets: &[FountainPacket],
    ) -> Result<Vec<u8>, FountainError> {
        let mut decoder = Decoder::new(metadata.inner.oti.clone());
        for packet in packets {
            let bytes = packet.as_bytes();
            if bytes.len() < 4 {
                return Err(FountainError::PacketTruncated { len: bytes.len() });
            }
            let encoding = EncodingPacket::deserialize(bytes);
            decoder.decode(encoding);
        }
        decoder
            .get_result()
            .ok_or(FountainError::InsufficientPackets)
    }
}
