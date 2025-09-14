use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A chunk of state updates delivered over the stream.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StateChunk {
    /// Monotonic sequence number.
    pub seq: u64,
    /// Optional latest chain height for lag detection.
    pub tip_height: u64,
    /// Updated account balances keyed by address.
    pub accounts: Vec<(String, u64)>,
    /// Merkle root of the accounts in this chunk.
    pub root: [u8; 32],
    /// Placeholder availability proof bytes.
    pub proof: Vec<u8>,
    /// Indicates if this chunk is a full snapshot compressed with zstd.
    pub compressed: bool,
}

/// Client-side helper maintaining a rolling cache of account state.
pub struct StateStream {
    cache: HashMap<String, u64>,
    next_seq: u64,
    lag_threshold: u64,
}

impl StateStream {
    /// Create a new stream with empty cache and default lag threshold of 8 blocks.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            next_seq: 0,
            lag_threshold: 8,
        }
    }

    /// Apply an incremental chunk of updates. Returns `Err(())` if the
    /// sequence number does not match the expected value or if the Merkle root
    /// is invalid.
    pub fn apply_chunk(&mut self, chunk: StateChunk) -> Result<(), ()> {
        if chunk.seq != self.next_seq {
            return Err(());
        }
        if self.compute_root(&chunk.accounts) != chunk.root {
            return Err(());
        }
        for (addr, bal) in chunk.accounts {
            self.cache.insert(addr, bal);
        }
        self.next_seq += 1;
        Ok(())
    }

    /// Apply a full snapshot, optionally compressed with zstd.
    pub fn apply_snapshot(&mut self, data: &[u8], compressed: bool) -> Result<(), std::io::Error> {
        let bytes = if compressed {
            zstd::decode_all(data)?
        } else {
            data.to_vec()
        };
        let map: HashMap<String, u64> = bincode::deserialize(&bytes).unwrap_or_default();
        self.cache = map;
        Ok(())
    }

    /// Returns true if the client is behind the provided chain height by more
    /// than the configured lag threshold.
    pub fn lagging(&self, tip_height: u64) -> bool {
        tip_height.saturating_sub(self.next_seq) > self.lag_threshold
    }

    fn compute_root(&self, accounts: &[(String, u64)]) -> [u8; 32] {
        let mut h = Hasher::new();
        for (addr, bal) in accounts.iter() {
            h.update(addr.as_bytes());
            h.update(&bal.to_le_bytes());
        }
        h.finalize().into()
    }
}
