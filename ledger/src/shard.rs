/// Identifier for a shard.
pub type ShardId = u16;

/// Minimal per-shard state placeholder.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
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
        let mut out = Vec::with_capacity(2 + self.state_root.len());
        out.extend_from_slice(&self.id.to_le_bytes());
        out.extend_from_slice(&self.state_root);
        out
    }

    /// Deserialize a shard state from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != 34 {
            return Err("invalid shard state payload".to_string());
        }
        let mut id_bytes = [0u8; 2];
        id_bytes.copy_from_slice(&bytes[..2]);
        let mut state_root = [0u8; 32];
        state_root.copy_from_slice(&bytes[2..]);
        Ok(Self {
            id: ShardId::from_le_bytes(id_bytes),
            state_root,
        })
    }
}
