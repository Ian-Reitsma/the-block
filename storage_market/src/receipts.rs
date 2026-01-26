//! Receipt emission for storage market settlements.

use crate::{ProofOutcome, ProofRecord};

/// Storage settlement receipt for block inclusion.
///
/// This matches the `StorageReceipt` structure in `node/src/receipts.rs` but is
/// defined here to avoid circular dependencies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageSettlementReceipt {
    pub contract_id: String,
    pub provider: String,
    pub bytes: u64,
    pub price: u64,
    pub block_height: u64,
    pub provider_escrow: u64,
    pub region: Option<String>,
    pub chunk_hash: Option<[u8; 32]>,
    pub signature_nonce: u64,
}

impl StorageSettlementReceipt {
    /// Create a receipt from a proof outcome for successful settlements.
    pub fn from_proof(
        proof: &ProofRecord,
        bytes: u64,
        price_per_block: u64,
        block_height: u64,
        region: Option<String>,
        chunk_hash: Option<[u8; 32]>,
    ) -> Option<Self> {
        // Only emit receipts for successful proofs with payment
        if proof.outcome == ProofOutcome::Success && proof.amount_accrued > 0 {
            Some(Self {
                contract_id: proof.object_id.clone(),
                provider: proof.provider_id.clone(),
                bytes,
                price: price_per_block,
                block_height,
                provider_escrow: proof.remaining_deposit,
                region,
                chunk_hash,
                signature_nonce: 0,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_created_for_successful_proof() {
        let proof = ProofRecord {
            object_id: "obj_123".into(),
            provider_id: "provider_a".into(),
            outcome: ProofOutcome::Success,
            slashed: 0,
            amount_accrued: 100,
            remaining_deposit: 900,
            proof_successes: 1,
            proof_failures: 0,
            chunk_hash: None,
        };

        let receipt = StorageSettlementReceipt::from_proof(&proof, 1024, 10, 100, None, None);
        assert!(receipt.is_some());

        let receipt = receipt.unwrap();
        assert_eq!(receipt.contract_id, "obj_123");
        assert_eq!(receipt.provider, "provider_a");
        assert_eq!(receipt.bytes, 1024);
        assert_eq!(receipt.price, 10);
        assert_eq!(receipt.block_height, 100);
        assert_eq!(receipt.provider_escrow, 900);
    }

    #[test]
    fn no_receipt_for_failed_proof() {
        let proof = ProofRecord {
            object_id: "obj_123".into(),
            provider_id: "provider_a".into(),
            outcome: ProofOutcome::Failure,
            slashed: 10,
            amount_accrued: 0,
            remaining_deposit: 890,
            proof_successes: 0,
            proof_failures: 1,
            chunk_hash: None,
        };

        let receipt = StorageSettlementReceipt::from_proof(&proof, 1024, 10, 100, None, None);
        assert!(receipt.is_none());
    }

    #[test]
    fn no_receipt_for_zero_payment() {
        let proof = ProofRecord {
            object_id: "obj_123".into(),
            provider_id: "provider_a".into(),
            outcome: ProofOutcome::Success,
            slashed: 0,
            amount_accrued: 0, // No payment
            remaining_deposit: 900,
            proof_successes: 1,
            proof_failures: 0,
            chunk_hash: None,
        };

        let receipt = StorageSettlementReceipt::from_proof(&proof, 1024, 10, 100, None, None);
        assert!(receipt.is_none());
    }
}
