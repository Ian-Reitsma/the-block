use serde::{Deserialize, Serialize};
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
    /// Next block height at which payment is due
    pub next_payment_block: u64,
    /// Total amount accrued so far
    pub accrued: u64,
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

    /// Deterministically derive the expected proof for a given chunk index.
    pub fn expected_proof(&self, chunk_idx: u64) -> [u8; 32] {
        use blake3::Hasher;
        let mut h = Hasher::new();
        h.update(self.object_id.as_bytes());
        h.update(&chunk_idx.to_le_bytes());
        h.finalize().into()
    }

    /// Verify a provider-supplied proof-of-retrievability.
    pub fn verify_proof(
        &self,
        chunk_idx: u64,
        proof: [u8; 32],
        current_block: u64,
    ) -> Result<(), ContractError> {
        self.is_active(current_block)?;
        if self.expected_proof(chunk_idx) == proof {
            Ok(())
        } else {
            Err(ContractError::ChallengeFailed)
        }
    }

    /// Accrue payments up to `current_block`, returning the amount due.
    pub fn pay(&mut self, current_block: u64) -> u64 {
        if current_block < self.next_payment_block {
            return 0;
        }
        let end = self.start_block + self.retention_blocks;
        if self.next_payment_block > end {
            return 0;
        }
        let due_block = current_block.min(end);
        let blocks = due_block - self.next_payment_block + 1;
        let amount = blocks * self.price_per_block;
        self.accrued += amount;
        self.next_payment_block = due_block + 1;
        amount
    }
}

#[cfg(test)]
mod tests {
    use super::StorageContract;

    #[test]
    fn payment_schedule_advances() {
        let mut c = StorageContract {
            object_id: "o".into(),
            provider_id: "p".into(),
            original_bytes: 0,
            shares: 0,
            price_per_block: 2,
            start_block: 0,
            retention_blocks: 5,
            next_payment_block: 1,
            accrued: 0,
        };
        assert_eq!(c.pay(0), 0);
        assert_eq!(c.pay(2), 4); // blocks 1-2
        assert_eq!(c.accrued, 4);
        assert_eq!(c.pay(10), 6); // blocks 3-5
        assert_eq!(c.accrued, 10);
    }
}
