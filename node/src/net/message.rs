use crate::net::peer::ReputationUpdate;
use crate::{p2p::handshake::Hello, BlobTx, Block, SignedTransaction};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Signed network message wrapper.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    /// Sender public key.
    pub pubkey: [u8; 32],
    /// Signature over the encoded body.
    pub signature: Vec<u8>,
    /// Inner message payload.
    pub body: Payload,
}

impl Message {
    /// Sign `body` with `kp` producing an authenticated message.
    pub fn new(body: Payload, sk: &SigningKey) -> Self {
        let bytes =
            bincode::serialize(&body).unwrap_or_else(|e| panic!("serialize message body: {e}"));
        let sig = sk.sign(&bytes);
        Self {
            pubkey: sk.verifying_key().to_bytes(),
            signature: sig.to_bytes().to_vec(),
            body,
        }
    }
}

/// Network message payloads exchanged between peers.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Payload {
    /// Version/feature negotiation and identity exchange.
    Handshake(Hello),
    /// Advertise known peers.
    Hello(Vec<SocketAddr>),
    /// Broadcast a transaction to be relayed and mined.
    Tx(SignedTransaction),
    /// Broadcast a blob transaction for inclusion in L2 blobspace.
    BlobTx(BlobTx),
    /// Broadcast a newly mined block.
    Block(Block),
    /// Share an entire chain snapshot for fork resolution.
    Chain(Vec<Block>),
    /// Disseminate a single erasure-coded shard of a blob.
    BlobChunk(BlobChunk),
    /// Propagate provider reputation scores.
    Reputation(Vec<ReputationUpdate>),
}

/// Individual erasure-coded shard associated with a blob root.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlobChunk {
    /// Commitment root this shard belongs to.
    pub root: [u8; 32],
    /// Index of this shard in the erasure-coded set.
    pub index: u32,
    /// Total number of shards.
    pub total: u32,
    /// Raw shard bytes.
    pub data: Vec<u8>,
}

// ReputationUpdate defined in peer.rs

/// Attempt to decode a [`Message`] from raw bytes.
#[cfg(feature = "fuzzy")]
#[allow(dead_code)]
pub fn decode(bytes: &[u8]) -> bincode::Result<Message> {
    bincode::deserialize(bytes)
}
