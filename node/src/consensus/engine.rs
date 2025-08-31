use super::{finality::FinalityGadget, unl::Unl};

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
        self.gadget.vote(validator, block_hash)
    }

    /// Roll back any finalized block.
    pub fn rollback(&mut self) {
        self.gadget.rollback();
    }
}
