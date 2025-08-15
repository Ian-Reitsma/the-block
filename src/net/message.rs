use crate::{Block, SignedTransaction};
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

/// Initial handshake information exchanged on connection.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Handshake {
    /// Opaque node identifier (usually the verifying key bytes).
    pub node_id: [u8; 32],
    /// Protocol version this node speaks.
    pub protocol_version: u32,
    /// Advertised optional features.
    pub features: Vec<String>,
}

/// Network message payloads exchanged between peers.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Payload {
    /// Version/feature negotiation and identity exchange.
    Handshake(Handshake),
    /// Advertise known peers.
    Hello(Vec<SocketAddr>),
    /// Broadcast a transaction to be relayed and mined.
    Tx(SignedTransaction),
    /// Broadcast a newly mined block.
    Block(Block),
    /// Share an entire chain snapshot for fork resolution.
    Chain(Vec<Block>),
}
