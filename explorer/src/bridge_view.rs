use crate::{BridgeChallengeRecord, Explorer};

pub fn active(explorer: &Explorer) -> Vec<BridgeChallengeRecord> {
    explorer
        .active_bridge_challenges()
        .unwrap_or_else(|_| Vec::new())
}
