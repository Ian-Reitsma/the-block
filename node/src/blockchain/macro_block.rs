use crate::util::binary_codec;
use foundation_serialization::{Deserialize, Serialize};
use ledger::address::ShardId;
use std::collections::HashMap;

/// Aggregated checkpoint summarising shard tips and rewards.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct MacroBlock {
    /// Height of the macro block (underlying micro-block height at emission).
    pub height: u64,
    /// Latest height per shard referenced by this macro block.
    pub shard_heights: HashMap<ShardId, u64>,
    /// State root per shard at this checkpoint.
    pub shard_roots: HashMap<ShardId, [u8; 32]>,
    /// Total consumer rewards accumulated since last macro block.
    pub reward_consumer: u64,
    /// Total industrial rewards accumulated since last macro block.
    pub reward_industrial: u64,
    /// Merkle root of the inter-shard message queue.
    pub queue_root: [u8; 32],
}

impl MacroBlock {
    /// RocksDB key for a given macro block height.
    pub fn db_key(height: u64) -> String {
        format!("macro:{height}")
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        binary_codec::serialize(self).expect("serialize macro block")
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, codec::Error> {
        binary_codec::deserialize(bytes)
    }
}
