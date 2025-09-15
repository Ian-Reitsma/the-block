use serde::{Deserialize, Serialize};

use bincode;

/// Identifier for a shard.
pub type ShardId = u16;

/// Minimal per-shard state placeholder.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub struct ShardState {
    /// Numeric shard identifier.
    pub id: ShardId,
    /// Root hash of the shard state.
    pub state_root: [u8; 32],
}

impl ShardState {
    /// Create a new shard state wrapper.
    pub fn new(id: ShardId, state_root: [u8; 32]) -> Self {
        Self { id, state_root }
    }

    /// Column family name for this shard.
    pub fn cf_name(id: ShardId) -> String {
        format!("shard:{id}")
    }

    /// Key within a shard's column family where the state root is stored.
    pub fn db_key() -> &'static str {
        "state"
    }

    /// Serialize the shard state to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialize shard state")
    }

    /// Deserialize a shard state from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> bincode::Result<Self> {
        bincode::deserialize(bytes)
    }
}
