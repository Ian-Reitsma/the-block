use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadinessStatement {
    pub window_secs: u64,
    pub min_unique_viewers: u64,
    pub min_host_count: u64,
    pub min_provider_count: u64,
    pub unique_viewers: u64,
    pub host_count: u64,
    pub provider_count: u64,
    pub ready: bool,
    pub last_updated: u64,
}

impl ReadinessStatement {
    pub fn commitment(&self, blinding: &[u8; 32]) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(&self.window_secs.to_le_bytes());
        h.update(&self.min_unique_viewers.to_le_bytes());
        h.update(&self.min_host_count.to_le_bytes());
        h.update(&self.min_provider_count.to_le_bytes());
        h.update(&self.unique_viewers.to_le_bytes());
        h.update(&self.host_count.to_le_bytes());
        h.update(&self.provider_count.to_le_bytes());
        h.update(&[self.ready as u8]);
        h.update(&self.last_updated.to_le_bytes());
        h.update(blinding);
        h.finalize().into()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadinessWitness {
    seed: [u8; 32],
}

impl ReadinessWitness {
    pub fn new(seed: [u8; 32]) -> Self {
        Self { seed }
    }

    pub fn seed(&self) -> &[u8; 32] {
        &self.seed
    }

    pub fn derive_blinding(&self, statement: &ReadinessStatement) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(self.seed());
        h.update(&statement.last_updated.to_le_bytes());
        h.update(&statement.unique_viewers.to_le_bytes());
        h.update(&statement.host_count.to_le_bytes());
        h.update(&statement.provider_count.to_le_bytes());
        h.update(&[statement.ready as u8]);
        h.finalize().into()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadinessPrivacyProof {
    commitment: [u8; 32],
    blinding: [u8; 32],
}

impl ReadinessPrivacyProof {
    pub fn commitment(&self) -> &[u8; 32] {
        &self.commitment
    }

    pub fn blinding(&self) -> &[u8; 32] {
        &self.blinding
    }

    pub fn blinding_mut(&mut self) -> &mut [u8; 32] {
        &mut self.blinding
    }
}

pub fn prove(statement: &ReadinessStatement, witness: &ReadinessWitness) -> ReadinessPrivacyProof {
    let blinding = witness.derive_blinding(statement);
    let commitment = statement.commitment(&blinding);
    ReadinessPrivacyProof {
        commitment,
        blinding,
    }
}

pub fn verify(statement: &ReadinessStatement, proof: &ReadinessPrivacyProof) -> bool {
    let expected = statement.commitment(proof.blinding());
    expected == *proof.commitment()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_commitment_round_trip() {
        let statement = ReadinessStatement {
            window_secs: 3_600,
            min_unique_viewers: 10,
            min_host_count: 5,
            min_provider_count: 4,
            unique_viewers: 12,
            host_count: 6,
            provider_count: 4,
            ready: true,
            last_updated: 77,
        };
        let witness = ReadinessWitness::new([7u8; 32]);
        let proof = prove(&statement, &witness);
        assert!(verify(&statement, &proof));
        let mut tampered = proof.clone();
        tampered.blinding[0] ^= 0xAA;
        assert!(!verify(&statement, &tampered));
    }
}
