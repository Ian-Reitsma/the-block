#[cfg(feature = "telemetry")]
use crate::BRIDGE_INVALID_PROOF_TOTAL;
use crate::{relayer::RelayerSet, Bridge, RelayerProof};

pub fn unlock(
    bridge: &mut Bridge,
    relayers: &mut RelayerSet,
    relayer: &str,
    user: &str,
    amount: u64,
    rproof: &RelayerProof,
) -> bool {
    if !rproof.verify(user, amount) {
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_INVALID_PROOF_TOTAL.inc();
        }
        relayers.slash(relayer, amount.min(1));
        return false;
    }
    let entry = bridge.locked.entry(user.to_string()).or_insert(0);
    if *entry < amount {
        #[cfg(feature = "telemetry")]
        {
            BRIDGE_INVALID_PROOF_TOTAL.inc();
        }
        relayers.slash(relayer, amount.min(1));
        return false;
    }
    *entry -= amount;
    true
}
