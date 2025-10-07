use crypto_suite::hashing::blake3::Hasher;
use std::collections::BTreeMap;

/// Merkle proof represented as sibling hashes with orientation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proof(pub Vec<([u8; 32], bool)>); // (sibling, is_left)

/// Simple in-memory Merkle trie backed by a `BTreeMap`.
#[derive(Debug, Clone, Default)]
pub struct MerkleTrie {
    pub(crate) map: BTreeMap<Vec<u8>, Vec<u8>>, // sorted for deterministic hashing
}

impl MerkleTrie {
    /// Create an empty trie.
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    /// Insert a key/value pair.
    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        self.map.insert(key.to_vec(), value.to_vec());
    }

    /// Fetch a value by key.
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.map.get(key).map(|v| v.as_slice())
    }

    /// Compute the root hash of the trie.
    pub fn root_hash(&self) -> [u8; 32] {
        let mut leaves: Vec<[u8; 32]> = self
            .map
            .iter()
            .map(|(k, v)| {
                let mut hasher = Hasher::new();
                hasher.update(k);
                hasher.update(v);
                hasher.finalize().into()
            })
            .collect();

        if leaves.is_empty() {
            return [0u8; 32];
        }

        while leaves.len() > 1 {
            let mut next = Vec::new();
            for chunk in leaves.chunks(2) {
                let mut hasher = Hasher::new();
                hasher.update(&chunk[0]);
                if chunk.len() == 2 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                }
                next.push(hasher.finalize().into());
            }
            leaves = next;
        }
        leaves[0]
    }

    /// Generate a Merkle proof for a given key.
    pub fn prove(&self, key: &[u8]) -> Option<Proof> {
        let mut index = self.map.keys().position(|k| k.as_slice() == key)?;
        let mut leaves: Vec<[u8; 32]> = self
            .map
            .iter()
            .map(|(k, v)| {
                let mut hasher = Hasher::new();
                hasher.update(k);
                hasher.update(v);
                hasher.finalize().into()
            })
            .collect();
        let mut proof = Vec::new();
        while leaves.len() > 1 {
            let sibling_index = if index % 2 == 0 { index + 1 } else { index - 1 };
            let sibling = if sibling_index < leaves.len() {
                leaves[sibling_index]
            } else {
                leaves[index]
            };
            let is_left = index % 2 == 1;
            proof.push((sibling, is_left));
            let mut next = Vec::new();
            for chunk in leaves.chunks(2) {
                let mut hasher = Hasher::new();
                hasher.update(&chunk[0]);
                if chunk.len() == 2 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                }
                next.push(hasher.finalize().into());
            }
            index /= 2;
            leaves = next;
        }
        Some(Proof(proof))
    }

    /// Verify a proof against a root hash.
    pub fn verify_proof(root: [u8; 32], key: &[u8], value: &[u8], proof: &Proof) -> bool {
        let mut hash: [u8; 32] = {
            let mut h = Hasher::new();
            h.update(key);
            h.update(value);
            h.finalize().into()
        };
        for (sibling, is_left) in &proof.0 {
            let mut h = Hasher::new();
            if *is_left {
                h.update(sibling);
                h.update(&hash);
            } else {
                h.update(&hash);
                h.update(sibling);
            }
            hash = h.finalize().into();
        }
        hash == root
    }
}
