use foundation_serialization::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", crate = "foundation_serialization::serde")]
pub enum EnergySettlementMode {
    Batch,
    RealTime,
}

impl crate::codec::BinaryCodec for EnergySettlementMode {
    fn encode(&self, writer: &mut crate::codec::BinaryWriter) {
        let value = match self {
            EnergySettlementMode::Batch => 0u8,
            EnergySettlementMode::RealTime => 1u8,
        };
        value.encode(writer);
    }

    fn decode(reader: &mut crate::codec::BinaryReader<'_>) -> crate::codec::Result<Self> {
        let value = u8::decode(reader)?;
        Ok(match value {
            0 => EnergySettlementMode::Batch,
            _ => EnergySettlementMode::RealTime,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct EnergySettlementChangePayload {
    pub desired_mode: EnergySettlementMode,
    pub activation_epoch: u64,
    pub rollback_window_epochs: u64,
    pub deps: Vec<u64>,
    #[serde(default)]
    pub memo: String,
    #[serde(default)]
    pub quorum_threshold_ppm: u32,
    #[serde(default)]
    pub expiry_blocks: u64,
}

impl Default for EnergySettlementChangePayload {
    fn default() -> Self {
        Self {
            desired_mode: EnergySettlementMode::RealTime,
            activation_epoch: 0,
            rollback_window_epochs: 1,
            deps: Vec::new(),
            memo: String::new(),
            quorum_threshold_ppm: 0,
            expiry_blocks: 0,
        }
    }
}

impl EnergySettlementChangePayload {
    pub fn validate(&self) -> Result<(), String> {
        if self.quorum_threshold_ppm > 1_000_000 {
            return Err("quorum_threshold_ppm must be <= 1_000_000".into());
        }
        if self.rollback_window_epochs == 0 {
            return Err("rollback_window_epochs must be > 0".into());
        }
        Ok(())
    }
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
