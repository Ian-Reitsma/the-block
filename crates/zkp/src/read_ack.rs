use crypto_suite::hashing::blake3::Hasher;
use foundation_serialization::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadAckStatement {
    pub manifest: [u8; 32],
    pub path_hash: [u8; 32],
    pub bytes: u64,
    pub timestamp: u64,
    pub client_hash: [u8; 32],
    pub domain: String,
    pub provider: String,
    pub campaign_id: Option<String>,
    pub creative_id: Option<String>,
}

impl ReadAckStatement {
    pub fn new(
        manifest: [u8; 32],
        path_hash: [u8; 32],
        bytes: u64,
        timestamp: u64,
        client_hash: [u8; 32],
        domain: String,
        provider: String,
        campaign_id: Option<String>,
        creative_id: Option<String>,
    ) -> Self {
        Self {
            manifest,
            path_hash,
            bytes,
            timestamp,
            client_hash,
            domain,
            provider,
            campaign_id,
            creative_id,
        }
    }

    pub fn commitment(
        &self,
        readiness_commitment: &[u8; 32],
        identity_commitment: &[u8; 32],
    ) -> [u8; 32] {
        let mut h = Hasher::new();
        h.update(&self.manifest);
        h.update(&self.path_hash);
        h.update(&self.bytes.to_le_bytes());
        h.update(&self.timestamp.to_le_bytes());
        h.update(self.domain.as_bytes());
        h.update(self.provider.as_bytes());
        if let Some(campaign) = &self.campaign_id {
            h.update(campaign.as_bytes());
        } else {
            h.update(&[0]);
        }
        if let Some(creative) = &self.creative_id {
            h.update(creative.as_bytes());
        } else {
            h.update(&[0]);
        }
        h.update(readiness_commitment);
        h.update(identity_commitment);
        h.finalize().into()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadAckWitness {
    identity_salt: [u8; 32],
}

impl ReadAckWitness {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            identity_salt: bytes,
        }
    }

    pub fn identity_salt(&self) -> &[u8; 32] {
        &self.identity_salt
    }

    pub fn derive_from_signature(sig: &[u8]) -> Self {
        let mut h = Hasher::new();
        h.update(sig);
        let mut salt = [0u8; 32];
        salt.copy_from_slice(&h.finalize().as_bytes()[..32]);
        Self::from_bytes(salt)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadAckPrivacyProof {
    ack_commitment: [u8; 32],
    identity_commitment: [u8; 32],
    readiness_commitment: [u8; 32],
    identity_salt: [u8; 32],
}

impl ReadAckPrivacyProof {
    pub fn ack_commitment(&self) -> &[u8; 32] {
        &self.ack_commitment
    }

    pub fn identity_commitment(&self) -> &[u8; 32] {
        &self.identity_commitment
    }

    pub fn readiness_commitment(&self) -> &[u8; 32] {
        &self.readiness_commitment
    }

    pub fn identity_salt(&self) -> &[u8; 32] {
        &self.identity_salt
    }
}

pub fn derive_identity_commitment(client_hash: &[u8; 32], witness: &ReadAckWitness) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(client_hash);
    h.update(witness.identity_salt());
    h.finalize().into()
}

pub fn prove(
    statement: &ReadAckStatement,
    readiness_commitment: &[u8; 32],
    witness: &ReadAckWitness,
) -> ReadAckPrivacyProof {
    let identity_commitment = derive_identity_commitment(&statement.client_hash, witness);
    let ack_commitment = statement.commitment(readiness_commitment, &identity_commitment);
    ReadAckPrivacyProof {
        ack_commitment,
        identity_commitment,
        readiness_commitment: *readiness_commitment,
        identity_salt: *witness.identity_salt(),
    }
}

pub fn verify(statement: &ReadAckStatement, proof: &ReadAckPrivacyProof) -> bool {
    let witness = ReadAckWitness::from_bytes(*proof.identity_salt());
    let expected_identity = derive_identity_commitment(&statement.client_hash, &witness);
    if expected_identity != *proof.identity_commitment() {
        return false;
    }
    let expected_commitment =
        statement.commitment(proof.readiness_commitment(), proof.identity_commitment());
    expected_commitment == *proof.ack_commitment()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commitments_round_trip() {
        let statement = ReadAckStatement::new(
            [1u8; 32],
            [2u8; 32],
            1024,
            77,
            [9u8; 32],
            "example.com".into(),
            "provider".into(),
            Some("cmp".into()),
            Some("creative".into()),
        );
        let readiness_commitment = [3u8; 32];
        let witness = ReadAckWitness::derive_from_signature(&[0xAB; 64]);
        let proof = prove(&statement, &readiness_commitment, &witness);
        assert!(verify(&statement, &proof));
        let mut tampered = proof.clone();
        tampered.identity_salt[0] ^= 0xFF;
        assert!(!verify(&statement, &tampered));
    }
}
