use super::{finality::FinalityGadget, unl::Unl};
#[cfg(feature = "telemetry")]
use crate::telemetry;

/// Federated consensus engine combining UNL and finality gadget.
pub struct ConsensusEngine {
    pub gadget: FinalityGadget,
}

impl ConsensusEngine {
    /// Create a new engine with an initial UNL.
    pub fn new(unl: Unl) -> Self {
        Self {
            gadget: FinalityGadget::new(unl),
        }
    }

    /// Cast a vote. Returns true if finalized.
    pub fn vote(&mut self, validator: &str, block_hash: &str) -> bool {
        let finalized = self.gadget.vote(validator, block_hash);
        #[cfg(feature = "telemetry")]
        telemetry::record_finality_snapshot(self.snapshot());
        finalized
    }

    /// Inspect the current voting state for auditing/tests.
    pub fn snapshot(&self) -> super::finality::FinalitySnapshot {
        self.gadget.snapshot()
    }

    /// Roll back any finalized block.
    pub fn rollback(&mut self) {
        self.gadget.rollback();
    }
}
