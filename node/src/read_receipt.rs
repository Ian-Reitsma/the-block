use crate::ad_readiness::AdReadinessSnapshot;
use ad_market::SelectionReceipt;
use crypto_suite::hashing::blake3::{self, Hasher};
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey};
use foundation_serialization::{Deserialize, Serialize};
use zkp::{
    read_ack::{self, ReadAckPrivacyProof, ReadAckStatement, ReadAckWitness},
    readiness,
};

/// Client-signed acknowledgement that a path was read from a manifest.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadAck {
    /// 32-byte manifest identifier.
    pub manifest: [u8; 32],
    /// Blake3 hash of the requested path.
    pub path_hash: [u8; 32],
    /// Number of bytes returned to the client.
    pub bytes: u64,
    /// Millisecond timestamp when the read completed.
    pub ts: u64,
    /// Salted hash of the client IP for audit sampling.
    pub client_hash: [u8; 32],
    /// Client public key used for signing.
    pub pk: [u8; 32],
    /// Ed25519 signature over the acknowledgement payload.
    #[serde(with = "foundation_serialization::serde_bytes")]
    pub sig: Vec<u8>,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Domain that served the content.
    pub domain: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Hosting/provider identifier resolved for the read.
    pub provider: String,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Optional advertising campaign matched for this impression.
    pub campaign_id: Option<String>,
    #[serde(default = "foundation_serialization::defaults::default")]
    /// Optional creative identifier returned by the ad marketplace.
    pub creative_id: Option<String>,
    #[serde(default)]
    /// Optional selection receipt attesting to the on-device auction outcome.
    pub selection_receipt: Option<SelectionReceipt>,
    #[serde(default)]
    /// Snapshot of readiness counters and proof bindings for this acknowledgement.
    pub readiness: Option<AdReadinessSnapshot>,
    #[serde(default)]
    /// Privacy proof binding this acknowledgement to its readiness commitment.
    pub zk_proof: Option<ReadAckPrivacyProof>,
}

impl ReadAck {
    /// Serialize fields and verify the signature against the embedded public key.
    pub fn verify(&self) -> bool {
        self.verify_signature() && self.verify_privacy()
    }

    /// Verify the Ed25519 signature embedded in this acknowledgement.
    pub fn verify_signature(&self) -> bool {
        let mut h = Hasher::new();
        h.update(&self.manifest);
        h.update(&self.path_hash);
        h.update(&self.bytes.to_le_bytes());
        h.update(&self.ts.to_le_bytes());
        h.update(&self.client_hash);
        let msg = h.finalize();
        let pk = match VerifyingKey::from_bytes(&self.pk) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let arr: [u8; 64] = match self.sig.as_slice().try_into() {
            Ok(a) => a,
            Err(_) => return false,
        };
        let sig = Signature::from_bytes(&arr);
        pk.verify(msg.as_bytes(), &sig).is_ok()
    }

    /// Compute the reservation discriminator to avoid collisions across identical reads.
    pub fn reservation_discriminator(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(&self.manifest);
        h.update(&self.path_hash);
        h.update(&self.ts.to_le_bytes());
        h.update(&self.client_hash);
        h.update(&self.sig);
        h.finalize().into()
    }

    fn privacy_statement(&self) -> ReadAckStatement {
        ReadAckStatement::new(
            self.manifest,
            self.path_hash,
            self.bytes,
            self.ts,
            self.client_hash,
            self.domain.clone(),
            self.provider.clone(),
            self.campaign_id.clone(),
            self.creative_id.clone(),
        )
    }

    /// Attach a privacy proof derived from the provided readiness snapshot.
    pub fn attach_privacy(&mut self, snapshot: AdReadinessSnapshot) {
        let readiness_commitment = match snapshot.zk_proof.as_ref() {
            Some(proof) => *proof.commitment(),
            None => {
                self.readiness = Some(snapshot);
                self.zk_proof = None;
                return;
            }
        };
        let witness = ReadAckWitness::derive_from_signature(&self.sig);
        let statement = self.privacy_statement();
        let proof = read_ack::prove(&statement, &readiness_commitment, &witness);
        self.readiness = Some(snapshot);
        self.zk_proof = Some(proof);
    }

    /// Verify the privacy proof if present.
    pub fn verify_privacy(&self) -> bool {
        match (&self.readiness, &self.zk_proof) {
            (Some(snapshot), Some(proof)) => match snapshot.zk_proof.as_ref() {
                Some(readiness_proof) => {
                    let statement = snapshot.to_statement();
                    if !readiness::verify(&statement, readiness_proof) {
                        return false;
                    }
                    let ack_statement = self.privacy_statement();
                    read_ack::verify(&ack_statement, proof)
                }
                None => false,
            },
            (None, None) => true,
            (None, Some(_)) | (Some(_), None) => false,
        }
    }
}

/// Batch of read acknowledgements represented by a Merkle root.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadBatch {
    /// Merkle root over all acknowledgements in this batch.
    pub root: [u8; 32],
    /// Total bytes served across acknowledgements.
    pub total_bytes: u64,
    /// Number of acknowledgements included.
    pub count: u32,
}

/// Simple in-memory batcher accumulating acknowledgements.
#[derive(Default)]
pub struct ReadBatcher {
    acks: Vec<ReadAck>,
}

impl ReadBatcher {
    pub fn new() -> Self {
        Self { acks: Vec::new() }
    }

    /// Push a new acknowledgement into the batcher.
    pub fn push(&mut self, ack: ReadAck) {
        self.acks.push(ack);
    }

    /// Compute a Merkle root and return the finalised batch, draining internal state.
    pub fn finalize(&mut self) -> ReadBatch {
        let mut leaves: Vec<[u8; 32]> = self.acks.iter().map(hash_ack).collect();
        let total_bytes = self.acks.iter().map(|a| a.bytes).sum();
        let count = leaves.len() as u32;
        if leaves.is_empty() {
            return ReadBatch {
                root: [0u8; 32],
                total_bytes: 0,
                count: 0,
            };
        }
        while leaves.len() > 1 {
            let mut next = Vec::with_capacity((leaves.len() + 1) / 2);
            for pair in leaves.chunks(2) {
                let mut h = Hasher::new();
                h.update(&pair[0]);
                if pair.len() == 2 {
                    h.update(&pair[1]);
                } else {
                    h.update(&pair[0]);
                }
                next.push(h.finalize().into());
            }
            leaves = next;
        }
        self.acks.clear();
        ReadBatch {
            root: leaves[0],
            total_bytes,
            count,
        }
    }
}

fn hash_ack(ack: &ReadAck) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(&ack.manifest);
    h.update(&ack.path_hash);
    h.update(&ack.bytes.to_le_bytes());
    h.update(&ack.ts.to_le_bytes());
    h.update(&ack.client_hash);
    h.update(&ack.pk);
    h.update(&ack.sig);
    h.update(ack.domain.as_bytes());
    h.update(ack.provider.as_bytes());
    if let Some(campaign) = &ack.campaign_id {
        h.update(campaign.as_bytes());
    } else {
        h.update(&[0u8]);
    }
    if let Some(creative) = &ack.creative_id {
        h.update(creative.as_bytes());
    } else {
        h.update(&[0u8]);
    }
    if let Some(snapshot) = &ack.readiness {
        h.update(&snapshot.window_secs.to_le_bytes());
        h.update(&snapshot.min_unique_viewers.to_le_bytes());
        h.update(&snapshot.min_host_count.to_le_bytes());
        h.update(&snapshot.min_provider_count.to_le_bytes());
        h.update(&snapshot.unique_viewers.to_le_bytes());
        h.update(&snapshot.host_count.to_le_bytes());
        h.update(&snapshot.provider_count.to_le_bytes());
        h.update(&[snapshot.ready as u8]);
        h.update(&snapshot.last_updated.to_le_bytes());
        for blocker in &snapshot.blockers {
            h.update(blocker.as_bytes());
        }
        if let Some(proof) = snapshot.zk_proof.as_ref() {
            h.update(proof.commitment());
            h.update(proof.blinding());
        } else {
            h.update(&[0u8]);
        }
    } else {
        h.update(&[0u8]);
    }
    if let Some(proof) = &ack.zk_proof {
        h.update(proof.ack_commitment());
        h.update(proof.identity_commitment());
        h.update(proof.readiness_commitment());
        h.update(proof.identity_salt());
    } else {
        h.update(&[0u8]);
    }
    h.finalize().into()
}
