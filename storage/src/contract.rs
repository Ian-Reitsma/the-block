use foundation_serialization::{Deserialize, Serialize};
use thiserror::Error;

use crate::merkle_proof::{verify_proof as verify_merkle_proof, MerkleProof, MerkleRoot};

/// StorageContract tracks file shards stored with a provider
/// along with payment schedule and erasure coding metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "foundation_serialization::serde")]
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
    /// Combined replica deposit currently reserved for the contract
    #[serde(default)]
    pub total_deposit: u64,
    /// Last block height where `pay` advanced the schedule
    #[serde(default)]
    pub last_payment_block: Option<u64>,
    /// Merkle root committing to all encoded chunks for this contract.
    ///
    /// This root is computed client-side at contract creation from the
    /// full set of chunks and stored on-chain. Providers must present
    /// Merkle proofs against this root to demonstrate actual data
    /// possession during challenges.
    pub storage_root: MerkleRoot,
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

    /// Verify a provider-supplied proof-of-retrievability using Merkle proofs.
    ///
    /// The provider must return the actual `chunk_data` alongside a Merkle
    /// proof that links the chunk to the on-chain `storage_root`. This
    /// prevents providers from computing proofs from metadata alone.
    pub fn verify_proof(
        &self,
        chunk_idx: u64,
        chunk_data: &[u8],
        proof: &MerkleProof,
        current_block: u64,
    ) -> Result<(), ContractError> {
        self.is_active(current_block)?;
        verify_merkle_proof(self.storage_root, chunk_idx, chunk_data, proof)
            .map_err(|_| ContractError::ChallengeFailed)
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
        self.last_payment_block = Some(due_block);
        amount
    }
}

#[cfg(test)]
mod tests {
    use super::StorageContract;
    use crate::merkle_proof::MerkleTree;

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
            total_deposit: 0,
            last_payment_block: None,
            storage_root: MerkleTree::build(&[b"chunk0".as_ref()])
                .expect("build tree")
                .root(),
        };
        assert_eq!(c.pay(0), 0);
        assert_eq!(c.pay(2), 4); // blocks 1-2
        assert_eq!(c.accrued, 4);
        assert_eq!(c.last_payment_block, Some(2));
        assert_eq!(c.pay(10), 6); // blocks 3-5
        assert_eq!(c.accrued, 10);
        assert_eq!(c.last_payment_block, Some(5));
    }
}
