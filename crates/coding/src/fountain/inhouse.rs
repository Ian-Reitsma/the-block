use super::{FountainBatch, FountainCoder, FountainError, FountainMetadata, FountainPacket};

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

    fn unavailable(&self) -> FountainError {
        FountainError::Encode(format!(
            "{ALG_LT_INHOUSE} fountain coder requires first-party RNG implementation"
        ))
    }
}

impl FountainCoder for InhouseLtFountain {
    fn algorithm(&self) -> &'static str {
        ALG_LT_INHOUSE
    }

    fn encode(&self, _data: &[u8]) -> Result<FountainBatch, FountainError> {
        Err(self.unavailable())
    }

    fn decode(
        &self,
        _metadata: &FountainMetadata,
        _packets: &[FountainPacket],
    ) -> Result<Vec<u8>, FountainError> {
        Err(FountainError::Decode(format!(
            "{ALG_LT_INHOUSE} decode requires first-party RNG implementation"
        )))
    }
}
