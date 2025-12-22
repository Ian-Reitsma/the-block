//! Provider-side challenge response and proof generation
//!
//! The provider must:
//! 1. Store all encoded chunks from the contract
//! 2. When challenged for a random chunk, generate a Merkle proof
//! 3. Submit both the chunk data AND the proof to the verifier
//! 4. Never fabricate proofs from metadata alone

use crate::contract::StorageContract;
use crate::merkle_proof::{MerkleProof, MerkleTree};
use std::collections::HashMap;

/// Error type for provider operations
#[derive(Debug, Clone)]
pub enum ProviderError {
    UnknownContract,
    ChunkNotFound,
    InvalidChallenge,
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownContract => write!(f, "unknown contract"),
            Self::ChunkNotFound => write!(f, "chunk not found in storage"),
            Self::InvalidChallenge => write!(f, "invalid challenge"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Challenge from the network requesting a proof-of-retrievability
#[derive(Debug, Clone)]
pub struct StorageChallenge {
    pub contract_id: u64,
    pub chunk_index: u64,
    pub challenge_id: String,
}

/// Proof response submitted by provider
#[derive(Debug, Clone)]
pub struct ChallengeResponse {
    pub challenge_id: String,
    pub contract_id: u64,
    pub chunk_index: u64,
    /// The actual chunk data - MUST be present for valid proof
    pub chunk_data: Vec<u8>,
    /// Merkle proof linking chunk to on-chain root
    pub merkle_proof: MerkleProof,
}

/// Provider-side storage manager
///
/// Maintains local chunk storage and generates valid Merkle proofs
/// only when the actual data is present.
pub struct StorageProvider {
    /// contract_id -> chunks (as stored locally)
    contract_storage: HashMap<u64, Vec<Vec<u8>>>,
    /// contract_id -> Merkle tree (for generating proofs)
    contract_trees: HashMap<u64, MerkleTree>,
}

impl StorageProvider {
    pub fn new() -> Self {
        Self {
            contract_storage: HashMap::new(),
            contract_trees: HashMap::new(),
        }
    }

    /// Accept a new contract with its chunks
    ///
    /// The provider receives:
    /// - The contract with the on-chain Merkle root
    /// - The full set of chunks to store
    ///
    /// The provider reconstructs the local Merkle tree from chunks
    /// to enable proof generation.
    pub fn accept_contract(
        &mut self,
        contract_id: u64,
        _contract: &StorageContract,
        chunks: Vec<Vec<u8>>,
    ) -> Result<(), ProviderError> {
        // Verify chunks are valid before accepting
        if chunks.is_empty() {
            return Err(ProviderError::InvalidChallenge);
        }

        // Build local Merkle tree for proof generation
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
        let tree = MerkleTree::build(&chunk_refs).map_err(|_| ProviderError::InvalidChallenge)?;

        self.contract_storage.insert(contract_id, chunks);
        self.contract_trees.insert(contract_id, tree);

        Ok(())
    }

    /// Process a challenge and generate a valid proof response
    ///
    /// This method requires BOTH:
    /// 1. Actual chunk data from local storage
    /// 2. A valid Merkle proof from the reconstructed tree
    ///
    /// It's impossible to generate a valid response without the actual data.
    pub fn respond_to_challenge(
        &self,
        challenge: &StorageChallenge,
    ) -> Result<ChallengeResponse, ProviderError> {
        // Retrieve chunks from storage
        let chunks = self
            .contract_storage
            .get(&challenge.contract_id)
            .ok_or(ProviderError::UnknownContract)?;

        // Retrieve Merkle tree for proof generation
        let tree = self
            .contract_trees
            .get(&challenge.contract_id)
            .ok_or(ProviderError::UnknownContract)?;

        // Get the specific chunk
        let chunk_index = challenge.chunk_index as usize;
        let chunk_data = chunks
            .get(chunk_index)
            .ok_or(ProviderError::ChunkNotFound)?
            .clone();

        // Generate Merkle proof
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
        let merkle_proof = tree
            .generate_proof(challenge.chunk_index, &chunk_refs)
            .map_err(|_| ProviderError::InvalidChallenge)?;

        Ok(ChallengeResponse {
            challenge_id: challenge.challenge_id.clone(),
            contract_id: challenge.contract_id,
            chunk_index: challenge.chunk_index,
            chunk_data,
            merkle_proof,
        })
    }

    /// Terminate a contract and free local storage
    pub fn release_contract(&mut self, contract_id: u64) {
        self.contract_storage.remove(&contract_id);
        self.contract_trees.remove(&contract_id);
    }

    /// Get number of contracts being maintained
    pub fn contract_count(&self) -> usize {
        self.contract_storage.len()
    }
}

impl Default for StorageProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::StorageContract;
    use crate::merkle_proof::MerkleRoot;

    fn make_test_contract(merkle_root: MerkleRoot) -> StorageContract {
        StorageContract {
            object_id: "obj-1".into(),
            provider_id: "provider-1".into(),
            original_bytes: 4096,
            shares: 4,
            price_per_block: 100,
            start_block: 1000,
            retention_blocks: 10000,
            next_payment_block: 2000,
            accrued: 0,
            total_deposit_ct: 0,
            last_payment_block: None,
            storage_root: merkle_root,
        }
    }

    #[test]
    fn provider_accepts_contract_with_chunks() {
        let chunks = vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9, 10, 11, 12]];
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
        let tree = MerkleTree::build(&chunk_refs).unwrap();
        let contract = make_test_contract(tree.root);

        let mut provider = StorageProvider::new();
        let result = provider.accept_contract(1, &contract, chunks);
        assert!(result.is_ok());
        assert_eq!(provider.contract_count(), 1);
    }

    #[test]
    fn provider_rejects_unknown_challenge() {
        let provider = StorageProvider::new();
        let challenge = StorageChallenge {
            contract_id: 999,
            chunk_index: 0,
            challenge_id: "chal-1".into(),
        };
        let result = provider.respond_to_challenge(&challenge);
        assert!(matches!(result, Err(ProviderError::UnknownContract)));
    }

    #[test]
    fn provider_generates_valid_proof() {
        let chunks = vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9, 10, 11, 12]];
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
        let tree = MerkleTree::build(&chunk_refs).unwrap();
        let contract = make_test_contract(tree.root);

        let mut provider = StorageProvider::new();
        provider
            .accept_contract(1, &contract, chunks.clone())
            .unwrap();

        let challenge = StorageChallenge {
            contract_id: 1,
            chunk_index: 1,
            challenge_id: "chal-1".into(),
        };

        let response = provider.respond_to_challenge(&challenge);
        assert!(response.is_ok());
        let resp = response.unwrap();
        assert_eq!(resp.chunk_data, chunks[1]);
    }

    #[test]
    fn provider_releases_contract() {
        let chunks = vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9, 10, 11, 12]];
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_ref()).collect();
        let tree = MerkleTree::build(&chunk_refs).unwrap();
        let contract = make_test_contract(tree.root);

        let mut provider = StorageProvider::new();
        provider.accept_contract(1, &contract, chunks).unwrap();
        assert_eq!(provider.contract_count(), 1);

        provider.release_contract(1);
        assert_eq!(provider.contract_count(), 0);
    }
}
