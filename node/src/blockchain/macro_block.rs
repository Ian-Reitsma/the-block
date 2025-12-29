use crate::ledger_binary;
use crate::util::binary_struct;
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
    /// Total BLOCK token rewards accumulated since last macro block (from all fee lanes).
    pub total_reward: u64,
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
        ledger_binary::encode_macro_block(self).expect("serialize macro block")
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> binary_struct::Result<Self> {
        ledger_binary::decode_macro_block(bytes)
    }
}
