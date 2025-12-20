//! Client-side storage contract creation with Merkle tree generation
//!
//! The client (uploader) is responsible for:
//! 1. Splitting data into chunks
//! 2. Building a Merkle tree over all chunks
//! 3. Sending the Merkle root to the provider in the contract
//! 4. Keeping chunks locally for future verification

use crate::contract::StorageContract;
use crate::merkle_proof::MerkleTree;
use foundation_serialization::Serialize;

/// Client-side contract initialization parameters
#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ContractCreationRequest {
    pub object_id: String,
    pub provider_id: String,
    pub original_bytes: u64,
    pub shares: u16,
    pub price_per_block: u64,
    pub start_block: u64,
    pub retention_blocks: u64,
    /// Merkle root computed from all encoded chunks
    pub merkle_root: Vec<u8>,
}

/// Result of contract creation on-chain
#[derive(Debug, Clone, Serialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct ContractCreationResult {
    pub contract_id: u64,
    pub contract: StorageContract,
}

/// Client-side storage contract builder
///
/// This builder helps ensure the client:
/// 1. Computes the correct Merkle tree from all chunks
/// 2. Submits the root to the provider
/// 3. Maintains the proof materials for future challenges
pub struct StorageContractBuilder {
    object_id: String,
    provider_id: String,
    original_bytes: u64,
    shares: u16,
    price_per_block: u64,
    start_block: u64,
    retention_blocks: u64,
    chunks: Vec<Vec<u8>>,
}

impl StorageContractBuilder {
    /// Create a new contract builder
    pub fn new(
        object_id: String,
        provider_id: String,
        original_bytes: u64,
        shares: u16,
        price_per_block: u64,
        start_block: u64,
        retention_blocks: u64,
    ) -> Self {
        Self {
            object_id,
            provider_id,
            original_bytes,
            shares,
            price_per_block,
            start_block,
            retention_blocks,
            chunks: Vec::new(),
        }
    }

    /// Add all encoded chunks for this contract
    ///
    /// The chunks should be the final erasure-coded output that will be
    /// stored with the provider. The Merkle tree is built over these chunks.
    pub fn with_chunks(mut self, chunks: Vec<Vec<u8>>) -> Self {
        self.chunks = chunks;
        self
    }

    /// Build the contract with Merkle tree root
    ///
    /// This method:
    /// 1. Constructs a Merkle tree over all chunks
    /// 2. Extracts the root for on-chain commitment
    /// 3. Returns both the contract creation request and the tree for verification
    pub fn build(self) -> Result<(ContractCreationRequest, MerkleTree), String> {
        if self.chunks.is_empty() {
            return Err("no chunks provided".into());
        }

        // Build Merkle tree from all chunks
        let chunk_refs: Vec<&[u8]> = self.chunks.iter().map(|chunk| chunk.as_ref()).collect();
        let tree = MerkleTree::build(&chunk_refs)
            .map_err(|e| format!("merkle tree construction failed: {}", e))?;

        let request = ContractCreationRequest {
            object_id: self.object_id,
            provider_id: self.provider_id,
            original_bytes: self.original_bytes,
            shares: self.shares,
            price_per_block: self.price_per_block,
            start_block: self.start_block,
            retention_blocks: self.retention_blocks,
            merkle_root: tree.root.as_bytes().to_vec(),
        };

        Ok((request, tree))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_requires_chunks() {
        let builder = StorageContractBuilder::new(
            "obj-1".into(),
            "provider-1".into(),
            1024,
            4,
            100,
            1000,
            10000,
        );
        let result = builder.build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no chunks"));
    }

    #[test]
    fn builder_creates_valid_request() {
        let chunks = vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9, 10, 11, 12]];
        let builder = StorageContractBuilder::new(
            "obj-1".into(),
            "provider-1".into(),
            1024,
            4,
            100,
            1000,
            10000,
        )
        .with_chunks(chunks);

        let result = builder.build();
        assert!(result.is_ok());
        let (request, _tree) = result.unwrap();
        assert_eq!(request.object_id, "obj-1");
        assert_eq!(request.provider_id, "provider-1");
        assert!(!request.merkle_root.is_empty());
    }

    #[test]
    fn merkle_root_is_consistent() {
        let chunks = vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9, 10, 11, 12]];

        let builder1 = StorageContractBuilder::new(
            "obj-1".into(),
            "provider-1".into(),
            1024,
            4,
            100,
            1000,
            10000,
        )
        .with_chunks(chunks.clone());

        let builder2 = StorageContractBuilder::new(
            "obj-1".into(),
            "provider-1".into(),
            1024,
            4,
            100,
            1000,
            10000,
        )
        .with_chunks(chunks);

        let (request1, _) = builder1.build().unwrap();
        let (request2, _) = builder2.build().unwrap();

        assert_eq!(request1.merkle_root, request2.merkle_root);
    }
}
