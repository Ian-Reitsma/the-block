use std::collections::HashMap;

#[derive(Default)]
pub struct ProofTracker {
    proofs: HashMap<Vec<u8>, u64>,
}

impl ProofTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, id: Vec<u8>, amount: u64) {
        *self.proofs.entry(id).or_default() += amount;
    }

    pub fn claim_all(&mut self) -> u64 {
        let total: u64 = self.proofs.values().copied().sum();
        self.proofs.clear();
        total
    }
}
