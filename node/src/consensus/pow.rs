use blake3::Hasher;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct BlockHeader {
    pub prev_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    /// Hash of the PoS checkpoint this PoW block builds upon.
    pub checkpoint_hash: [u8; 32],
    pub nonce: u64,
    pub difficulty: u64,
    pub timestamp: u64,
}

impl BlockHeader {
    pub fn hash(&self) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(&self.prev_hash);
        h.update(&self.merkle_root);
        h.update(&self.checkpoint_hash);
        h.update(&self.nonce.to_le_bytes());
        h.update(&self.timestamp.to_le_bytes());
        h.finalize().into()
    }
}

fn target(difficulty: u64) -> u64 {
    u64::MAX / difficulty.max(1)
}

pub fn mine(mut header: BlockHeader) -> BlockHeader {
    loop {
        let hash = header.hash();
        let value = u64::from_le_bytes(hash[..8].try_into().unwrap_or_default());
        if value <= target(header.difficulty) {
            return header;
        }
        header.nonce = header.nonce.wrapping_add(1);
    }
}

/// Adjust difficulty based on elapsed time.
pub fn adjust_difficulty(prev: u64, actual_secs: u64, target_secs: u64) -> u64 {
    let mut next = prev.saturating_mul(target_secs.max(1)) / actual_secs.max(1);
    let min = prev / 4;
    let max = prev * 4;
    if next < min {
        next = min;
    }
    if next > max {
        next = max;
    }
    next.max(1)
}

/// Helper to build a header template with current time.
pub fn template(
    prev_hash: [u8; 32],
    merkle_root: [u8; 32],
    checkpoint_hash: [u8; 32],
    difficulty: u64,
) -> BlockHeader {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();
    BlockHeader {
        prev_hash,
        merkle_root,
        checkpoint_hash,
        nonce: 0,
        difficulty,
        timestamp: ts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mines_block() {
        let header = template([0u8; 32], [1u8; 32], [2u8; 32], 1_000_000);
        let mined = mine(header.clone());
        let hash = mined.hash();
        let value = u64::from_le_bytes(hash[..8].try_into().unwrap_or_default());
        assert!(value <= target(header.difficulty));
    }

    #[test]
    fn difficulty_adjusts() {
        let prev = 1000;
        let next = adjust_difficulty(prev, 240, 120); // twice the target time -> easier
        assert!(next < prev);
    }
}
