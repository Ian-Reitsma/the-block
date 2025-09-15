use std::collections::HashMap;

use crate::{Block, TokenAmount};

#[derive(Default)]
pub struct ProofTracker {
    proofs: HashMap<Vec<u8>, u64>,
}

impl ProofTracker {
    pub fn new() -> Self {
        Self {
            proofs: HashMap::new(),
        }
    }

    /// Record a proof relay from `id` worth `amount` CT micro-rebate.
    pub fn record(&mut self, id: Vec<u8>, amount: u64) {
        *self.proofs.entry(id).or_default() += amount;
    }

    /// Claim all pending rebates, zeroing the tracker.
    pub fn claim_all(&mut self) -> u64 {
        let total: u64 = self.proofs.values().sum();
        if total > 0 {
            #[cfg(feature = "telemetry")]
            {
                crate::telemetry::PROOF_REBATES_CLAIMED_TOTAL.inc();
                crate::telemetry::PROOF_REBATES_AMOUNT_TOTAL.inc_by(total);
            }
        }
        self.proofs.clear();
        total
    }
}

/// Apply `amount` rebates to block coinbase.
pub fn apply_rebates(block: &mut Block, amount: u64) {
    if amount > 0 {
        block.coinbase_consumer = block
            .coinbase_consumer
            .saturating_add(TokenAmount::new(amount));
    }
}
