use super::constants::DIFFICULTY_WINDOW;
use super::difficulty;
use crate::range_boost;
use blake3::Hasher;
#[cfg(feature = "quantum")]
use crypto::dilithium;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct BlockHeader {
    pub prev_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    /// Hash of the PoS checkpoint this PoW block builds upon.
    pub checkpoint_hash: [u8; 32],
    pub nonce: u64,
    pub difficulty: u64,
    /// Global base fee in effect for this header.
    pub base_fee: u64,
    pub timestamp_millis: u64,
    #[cfg(feature = "quantum")]
    pub dilithium_pubkey: Vec<u8>,
    #[cfg(feature = "quantum")]
    pub dilithium_sig: Vec<u8>,
    /// Merkle/KZG roots for L2 blob commitments anchored in this block.
    pub l2_roots: Vec<[u8; 32]>,
    /// Total byte sizes per L2 root for accounting.
    pub l2_sizes: Vec<u32>,
    /// Commitment to VDF preimage for randomness fuse.
    pub vdf_commit: [u8; 32],
    /// VDF output revealed for the commitment from two blocks prior.
    pub vdf_output: [u8; 32],
    /// Optional proof bytes (Pietrzak recursive proof).
    pub vdf_proof: Vec<u8>,
}

impl BlockHeader {
    pub fn hash(&self) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(&self.prev_hash);
        h.update(&self.merkle_root);
        h.update(&self.checkpoint_hash);
        h.update(&self.nonce.to_le_bytes());
        h.update(&self.difficulty.to_le_bytes());
        h.update(&self.base_fee.to_le_bytes());
        h.update(&self.timestamp_millis.to_le_bytes());
        h.update(&(self.l2_roots.len() as u32).to_le_bytes());
        for r in &self.l2_roots {
            h.update(r);
        }
        h.update(&(self.l2_sizes.len() as u32).to_le_bytes());
        for s in &self.l2_sizes {
            h.update(&s.to_le_bytes());
        }
        h.update(&self.vdf_commit);
        h.update(&self.vdf_output);
        h.update(&(self.vdf_proof.len() as u32).to_le_bytes());
        h.update(&self.vdf_proof);
        h.finalize().into()
    }

    #[cfg(feature = "quantum")]
    pub fn verify_dilithium(&self) -> bool {
        if self.dilithium_pubkey.is_empty() || self.dilithium_sig.is_empty() {
            return false;
        }
        let msg = self.hash();
        dilithium::verify(&self.dilithium_pubkey, &msg, &self.dilithium_sig)
    }
}

fn target(difficulty: u64) -> u64 {
    u64::MAX / difficulty.max(1)
}

fn solve(mut header: BlockHeader, tgt: u64) -> BlockHeader {
    loop {
        if range_boost::mesh_active() {
            thread::sleep(Duration::from_millis(10));
        }
        let hash = header.hash();
        let value = u64::from_le_bytes(hash[..8].try_into().unwrap_or_default());
        if value <= tgt {
            return header;
        }
        header.nonce = header.nonce.wrapping_add(1);
    }
}

/// Stateful PoW miner that tracks difficulty and recent timestamps.
pub struct Miner {
    difficulty: u64,
    target_millis: u64,
    timestamps: Vec<u64>,
    target_value: u64,
}

impl Miner {
    /// Create a new miner with an initial difficulty and target spacing in milliseconds.
    pub fn new(initial_difficulty: u64, target_millis: u64) -> Self {
        let diff = initial_difficulty.max(1);
        Self {
            difficulty: diff,
            target_millis,
            timestamps: Vec::new(),
            target_value: target(diff),
        }
    }

    /// Access the difficulty that will be used for the next mined block.
    pub fn difficulty(&self) -> u64 {
        self.difficulty
    }

    /// Mine a block header and update the internal difficulty based on elapsed time.
    pub fn mine(&mut self, mut header: BlockHeader) -> BlockHeader {
        #[cfg(feature = "telemetry")]
        let span = if crate::telemetry::should_log("consensus") {
            Some(crate::log_context!(
                block = self.timestamps.len() as u64 + 1
            ))
        } else {
            None
        };
        #[cfg(feature = "telemetry")]
        if let Some(ref s) = span {
            tracing::info!(parent: s, "pow_start");
        }
        header.difficulty = self.difficulty;
        let mined = solve(header, self.target_value);
        #[cfg(feature = "telemetry")]
        if let Some(s) = span {
            tracing::info!(parent: &s, nonce = mined.nonce, "pow_end");
        }
        self.timestamps.push(mined.timestamp_millis);
        if self.timestamps.len() > DIFFICULTY_WINDOW {
            let excess = self.timestamps.len() - DIFFICULTY_WINDOW;
            self.timestamps.drain(0..excess);
        }
        self.difficulty =
            difficulty::retarget(self.difficulty, &self.timestamps, self.target_millis);
        self.target_value = target(self.difficulty);
        mined
    }
}

/// Adjust difficulty based on elapsed time.

/// Helper to build a header template with current time.
pub fn template(
    prev_hash: [u8; 32],
    merkle_root: [u8; 32],
    checkpoint_hash: [u8; 32],
    difficulty: u64,
    base_fee: u64,
) -> BlockHeader {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_millis() as u64;
    BlockHeader {
        prev_hash,
        merkle_root,
        checkpoint_hash,
        nonce: 0,
        difficulty,
        base_fee,
        timestamp_millis: ts,
        #[cfg(feature = "quantum")]
        dilithium_pubkey: Vec::new(),
        #[cfg(feature = "quantum")]
        dilithium_sig: Vec::new(),
        l2_roots: Vec::new(),
        l2_sizes: Vec::new(),
        vdf_commit: [0u8; 32],
        vdf_output: [0u8; 32],
        vdf_proof: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mines_block() {
        let header = template([0u8; 32], [1u8; 32], [2u8; 32], 1_000_000, 1);
        let mut miner = Miner::new(1_000_000, 1_000);
        let mined = miner.mine(header.clone());
        let hash = mined.hash();
        let value = u64::from_le_bytes(hash[..8].try_into().unwrap_or_default());
        assert!(value <= target(header.difficulty));
    }

    #[test]
    fn difficulty_decreases_when_blocks_slow() {
        let mut miner = Miner::new(1_000, 1_000);
        let mut h1 = template([0u8; 32], [1u8; 32], [2u8; 32], miner.difficulty(), 1);
        h1.timestamp_millis = 0;
        let _b1 = miner.mine(h1);
        let mut h2 = template([0u8; 32], [1u8; 32], [2u8; 32], miner.difficulty(), 1);
        h2.timestamp_millis = 3_000;
        let _b2 = miner.mine(h2);
        let mut h3 = template([0u8; 32], [1u8; 32], [2u8; 32], miner.difficulty(), 1);
        h3.timestamp_millis = 4_000;
        let b3 = miner.mine(h3);
        assert!(b3.difficulty < 1_000);
        assert!(miner.difficulty() <= b3.difficulty);
    }

    #[test]
    fn difficulty_increases_when_blocks_fast() {
        let mut miner = Miner::new(1_000, 1_000);
        let mut h1 = template([0u8; 32], [1u8; 32], [2u8; 32], miner.difficulty(), 1);
        h1.timestamp_millis = 0;
        let _b1 = miner.mine(h1);
        let mut h2 = template([0u8; 32], [1u8; 32], [2u8; 32], miner.difficulty(), 1);
        h2.timestamp_millis = 500;
        let _b2 = miner.mine(h2);
        let mut h3 = template([0u8; 32], [1u8; 32], [2u8; 32], miner.difficulty(), 1);
        h3.timestamp_millis = 1_000;
        let b3 = miner.mine(h3);
        assert!(b3.difficulty > 1_000);
    }
}
