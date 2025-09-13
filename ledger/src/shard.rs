use serde::{Deserialize, Serialize};

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
}
