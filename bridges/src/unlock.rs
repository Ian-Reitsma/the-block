#[cfg(feature = "telemetry")]
use crate::BRIDGE_INVALID_PROOF_TOTAL;
use crate::{relayer::RelayerSet, Bridge, PendingWithdrawal, RelayerBundle};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn unlock(
    bridge: &mut Bridge,
    relayers: &mut RelayerSet,
    relayer: &str,
    user: &str,
    amount: u64,
    bundle: &RelayerBundle,
) -> bool {
    let (valid, invalid) = bundle.verify(user, amount);
    for rel in invalid {
        relayers.slash(&rel, 1);
    }
    if valid < bridge.cfg.relayer_quorum || !bundle.relayer_ids().iter().any(|id| id == relayer) {
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
    let commitment = bundle.aggregate_commitment(user, amount);
    if bridge.pending_withdrawals.contains_key(&commitment) {
        return false;
    }
    *entry -= amount;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    bridge.pending_withdrawals.insert(
        commitment,
        PendingWithdrawal {
            user: user.to_string(),
            amount,
            relayers: bundle.relayer_ids(),
            initiated_at: now,
            challenged: false,
        },
    );
    true
}
