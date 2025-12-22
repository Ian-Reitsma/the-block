//! Merkle Proof of Retrievability System
//!
//! Implements cryptographic proof-of-storage using Merkle trees.
//! Providers must demonstrate possession of actual data chunks to earn payment.
//! This prevents the attack where providers compute proofs from metadata alone.

use crypto_suite::hashing::blake3;
use foundation_serialization::{Deserialize, Serialize};

/// Merkle tree root hash for a stored object
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MerkleRoot([u8; 32]);

impl MerkleRoot {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn new(bytes: [u8; 32]) -> Self {
        MerkleRoot(bytes)
    }
}

const MAX_MERKLE_DEPTH: u16 = 21;
const MAX_LEAF_COUNT: usize = 1 << (MAX_MERKLE_DEPTH - 1);

/// Merkle proof path from leaf to root
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MerkleProof {
    /// Sibling hashes from leaf to root
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub path: Vec<u8>,
    /// Number of hashes in path (each 32 bytes)
    pub path_len: u16,
}

impl MerkleProof {
    pub fn new(path: Vec<u8>) -> Result<Self, MerkleError> {
        if path.len() % 32 != 0 {
            return Err(MerkleError::InvalidTreeStructure {
                reason: "Merkle proof path must be multiple of 32 bytes".into(),
            });
        }
        let path_len = (path.len() / 32) as u16;
        if path_len > MAX_MERKLE_DEPTH {
            return Err(MerkleError::InvalidProofLength {
                expected: MAX_MERKLE_DEPTH,
                got: path_len,
            });
        }
        Ok(Self { path, path_len })
    }

    /// Get individual sibling hash at position
    pub fn get_sibling(&self, idx: usize) -> Option<[u8; 32]> {
        if idx >= self.path_len as usize {
            return None;
        }
        let start = idx * 32;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&self.path[start..start + 32]);
        Some(hash)
    }
}

/// Error types for Merkle proof operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub enum MerkleError {
    /// Proof verification failed
    VerificationFailed { reason: String },
    /// Proof path length doesn't match tree depth
    InvalidProofLength { expected: u16, got: u16 },
    /// Chunk index out of bounds
    ChunkIndexOutOfBounds { index: u64, max: u64 },
    /// Invalid merkle tree structure
    InvalidTreeStructure { reason: String },
}

impl std::fmt::Display for MerkleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VerificationFailed { reason } => write!(f, "Verification failed: {}", reason),
            Self::InvalidProofLength { expected, got } => {
                write!(
                    f,
                    "Proof length mismatch: expected {}, got {}",
                    expected, got
                )
            }
            Self::ChunkIndexOutOfBounds { index, max } => {
                write!(f, "Chunk index {} out of bounds (max: {})", index, max)
            }
            Self::InvalidTreeStructure { reason } => {
                write!(f, "Invalid tree structure: {}", reason)
            }
        }
    }
}

impl std::error::Error for MerkleError {}

/// Merkle tree for storage proof
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "foundation_serialization::serde")]
pub struct MerkleTree {
    /// Root hash of the tree
    pub root: MerkleRoot,
    /// Depth of tree (for optimization)
    pub depth: u16,
    /// Number of leaf nodes
    pub leaf_count: u64,
    #[serde(skip)]
    leaf_hashes: Vec<[u8; 32]>,
}

impl MerkleTree {
    /// Build a Merkle tree from chunks
    /// Chunks are hashed with: BLAKE3("chunk" || chunk_data || index)
    pub fn build(chunks: &[&[u8]]) -> Result<Self, MerkleError> {
        if chunks.is_empty() {
            return Err(MerkleError::InvalidTreeStructure {
                reason: "cannot build tree from empty chunks".into(),
            });
        }

        if chunks.len() > MAX_LEAF_COUNT {
            return Err(MerkleError::InvalidTreeStructure {
                reason: format!(
                    "chunk count {} exceeds maximum allowed ({})",
                    chunks.len(),
                    MAX_LEAF_COUNT
                ),
            });
        }

        let leaf_count = chunks.len() as u64;
        let depth = (leaf_count.next_power_of_two().ilog2()) as u16 + 1;

        if depth > MAX_MERKLE_DEPTH {
            return Err(MerkleError::InvalidTreeStructure {
                reason: format!(
                    "tree depth {} exceeds limit of {} levels",
                    depth, MAX_MERKLE_DEPTH
                ),
            });
        }

        // Build leaves
        let leaf_hashes: Vec<[u8; 32]> = chunks
            .iter()
            .enumerate()
            .map(|(idx, chunk)| {
                let mut h = blake3::Hasher::new();
                h.update(b"chunk");
                h.update(chunk);
                h.update(&(idx as u64).to_le_bytes());
                let hash: [u8; 32] = h.finalize().into();
                hash
            })
            .collect();
        let mut level = leaf_hashes.clone();

        // Pad to next power of 2
        let target_size = (leaf_count as f64).log2().ceil() as u32;
        let padded_size = 1u64 << target_size;
        while level.len() < padded_size as usize {
            level.push([0u8; 32]);
        }

        // Build tree bottom-up
        while level.len() > 1 {
            let mut next_level = Vec::new();
            for i in (0..level.len()).step_by(2) {
                let left = level[i];
                let right = if i + 1 < level.len() {
                    level[i + 1]
                } else {
                    [0u8; 32]
                };

                let mut h = blake3::Hasher::new();
                h.update(b"parent");
                h.update(&left);
                h.update(&right);
                let hash: [u8; 32] = h.finalize().into();
                next_level.push(hash);
            }
            level = next_level;
        }

        let root = MerkleRoot(level[0]);
        Ok(MerkleTree {
            root,
            depth,
            leaf_count,
            leaf_hashes,
        })
    }

    /// Return the root hash of the tree
    pub fn root(&self) -> MerkleRoot {
        self.root
    }

    /// Generate a proof for a specific chunk index
    pub fn generate_proof(
        &self,
        chunk_idx: u64,
        chunks: &[&[u8]],
    ) -> Result<MerkleProof, MerkleError> {
        if chunk_idx >= self.leaf_count {
            return Err(MerkleError::ChunkIndexOutOfBounds {
                index: chunk_idx,
                max: self.leaf_count - 1,
            });
        }

        if chunks.len() as u64 != self.leaf_count {
            return Err(MerkleError::InvalidTreeStructure {
                reason: format!("expected {} chunks, got {}", self.leaf_count, chunks.len()),
            });
        }

        let idx_usize = chunk_idx as usize;
        let mut leaf_hasher = blake3::Hasher::new();
        leaf_hasher.update(b"chunk");
        leaf_hasher.update(chunks[idx_usize]);
        leaf_hasher.update(&chunk_idx.to_le_bytes());
        let chunk_hash: [u8; 32] = leaf_hasher.finalize().into();

        if chunk_hash != self.leaf_hashes[idx_usize] {
            return Err(MerkleError::InvalidTreeStructure {
                reason: "chunk data does not match cached leaf hash".into(),
            });
        }

        let mut level = self.leaf_hashes.clone();

        // Pad to next power of 2
        let target_size = (self.leaf_count as f64).log2().ceil() as u32;
        let padded_size = 1u64 << target_size;
        while level.len() < padded_size as usize {
            level.push([0u8; 32]);
        }

        // Collect proof path
        let mut proof_path = Vec::new();
        let mut current_idx = idx_usize;

        while level.len() > 1 {
            let sibling_idx = if current_idx % 2 == 0 {
                current_idx + 1
            } else {
                current_idx - 1
            };

            if sibling_idx < level.len() {
                proof_path.extend_from_slice(&level[sibling_idx]);
            } else {
                proof_path.extend_from_slice(&[0u8; 32]);
            }

            // Build next level
            let mut next_level = Vec::new();
            for i in (0..level.len()).step_by(2) {
                let left = level[i];
                let right = if i + 1 < level.len() {
                    level[i + 1]
                } else {
                    [0u8; 32]
                };

                let mut h = blake3::Hasher::new();
                h.update(b"parent");
                h.update(&left);
                h.update(&right);
                let hash: [u8; 32] = h.finalize().into();
                next_level.push(hash);
            }

            current_idx /= 2;
            level = next_level;
        }

        MerkleProof::new(proof_path)
    }
}

/// Verify a Merkle proof given the root and leaf data
pub fn verify_proof(
    root: MerkleRoot,
    chunk_idx: u64,
    chunk_data: &[u8],
    proof: &MerkleProof,
) -> Result<(), MerkleError> {
    // Compute leaf hash
    let mut h = blake3::Hasher::new();
    h.update(b"chunk");
    h.update(chunk_data);
    h.update(&chunk_idx.to_le_bytes());
    let mut current_hash: [u8; 32] = h.finalize().into();

    // Verify path to root
    let mut idx = chunk_idx;
    for i in 0..proof.path_len as usize {
        let sibling = proof
            .get_sibling(i)
            .ok_or_else(|| MerkleError::VerificationFailed {
                reason: format!("missing sibling at index {}", i),
            })?;

        let mut h = blake3::Hasher::new();
        h.update(b"parent");

        if idx % 2 == 0 {
            h.update(&current_hash);
            h.update(&sibling);
        } else {
            h.update(&sibling);
            h.update(&current_hash);
        }

        current_hash = h.finalize().into();
        idx /= 2;
    }

    if current_hash == *root.as_bytes() {
        Ok(())
    } else {
        Err(MerkleError::VerificationFailed {
            reason: "computed root does not match expected root".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_verify_merkle_tree() {
        let chunks = vec![b"chunk0", b"chunk1", b"chunk2", b"chunk3"];
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|c| c.as_ref()).collect();

        let tree = MerkleTree::build(&chunk_refs).expect("build tree");
        assert_eq!(tree.leaf_count, 4);

        // Generate and verify proof for chunk 1
        let proof = tree.generate_proof(1, &chunk_refs).expect("generate proof");
        let result = verify_proof(tree.root, 1, b"chunk1", &proof);
        assert!(result.is_ok());
    }

    #[test]
    fn invalid_chunk_data_rejected() {
        let chunks = vec![b"chunk0", b"chunk1", b"chunk2", b"chunk3"];
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|c| c.as_ref()).collect();

        let tree = MerkleTree::build(&chunk_refs).expect("build tree");
        let proof = tree.generate_proof(1, &chunk_refs).expect("generate proof");

        // Try to verify with different data
        let result = verify_proof(tree.root, 1, b"wrong_data", &proof);
        assert!(result.is_err());
    }

    #[test]
    fn chunk_index_out_of_bounds() {
        let chunks = vec![b"chunk0", b"chunk1"];
        let chunk_refs: Vec<&[u8]> = chunks.iter().map(|c| c.as_ref()).collect();

        let tree = MerkleTree::build(&chunk_refs).expect("build tree");
        let result = tree.generate_proof(10, &chunk_refs);
        assert!(matches!(
            result,
            Err(MerkleError::ChunkIndexOutOfBounds { .. })
        ));
    }
}
