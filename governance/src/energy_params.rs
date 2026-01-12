use foundation_serialization::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", crate = "foundation_serialization::serde")]
pub enum EnergySettlementMode {
    Batch,
    RealTime,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergySettlementPayload {
    pub mode: EnergySettlementMode,
    pub quorum_threshold_ppm: u32,
    pub expiry_blocks: u64,
}

impl Default for EnergySettlementPayload {
    fn default() -> Self {
        Self {
            mode: EnergySettlementMode::RealTime,
            quorum_threshold_ppm: 0,
            expiry_blocks: 0,
        }
    }
}

impl EnergySettlementPayload {
    pub fn validate(&self) -> Result<(), String> {
        if self.quorum_threshold_ppm > 1_000_000 {
            return Err("quorum_threshold_ppm must be <= 1_000_000".into());
        }
        Ok(())
    }
}
