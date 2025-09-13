use serde::{Serialize, Deserialize};
use thiserror::Error;

/// StorageContract tracks file shards stored with a provider
/// along with payment schedule and erasure coding metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageContract {
    /// Identifier of the stored object
    pub object_id: String,
    /// Provider storing the data
    pub provider_id: String,
    /// Total original bytes before erasure coding
    pub original_bytes: u64,
    /// Number of encoded shares
    pub shares: u16,
    /// Price per block agreed upon
    pub price_per_block: u64,
    /// Starting block height
    pub start_block: u64,
    /// Blocks of retention
    pub retention_blocks: u64,
}

/// Errors related to storage contracts
#[derive(Debug, Error)]
pub enum ContractError {
    #[error("contract expired")]
    Expired,
    #[error("challenge failed")]
    ChallengeFailed,
}

impl StorageContract {
    /// Checks whether the contract is still valid at given block height.
    pub fn is_active(&self, current_block: u64) -> Result<(), ContractError> {
        if current_block > self.start_block + self.retention_blocks {
            Err(ContractError::Expired)
        } else {
            Ok(())
        }
    }
}
