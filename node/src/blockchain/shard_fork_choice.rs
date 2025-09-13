use ledger::address::ShardId;

/// Select the best PoW tip for a shard.
///
/// This is currently a placeholder and returns `None`.
pub fn select_tip(_shard: ShardId) -> Option<[u8; 32]> {
    None
}
