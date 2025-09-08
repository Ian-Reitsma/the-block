use blake3::Hasher;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_bytes;

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
    #[serde(with = "serde_bytes")]
    pub sig: Vec<u8>,
}

impl ReadAck {
    /// Serialize fields and verify the signature against the embedded public key.
    pub fn verify(&self) -> bool {
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
    h.finalize().into()
}
