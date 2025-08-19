use serde::{Deserialize, Serialize};

/// Feature bits advertised during peer handshakes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeatureBits(pub u32);

impl FeatureBits {
    /// P2P protocol supporting future fee routing.
    pub const FEE_ROUTING_V2: u32 = 0x0004;
}

/// Initial handshake exchanged between peers prior to gossip.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Handshake {
    pub version: u32,
    pub features: u32,
}

/// Messages exchanged between peers once a connection is established.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WireMessage {
    /// Broadcast a serialized transaction to peers.
    TxBroadcast { tx: Vec<u8> },
    /// Announce a newly mined block to peers.
    BlockAnnounce { block: Vec<u8> },
    /// Request headers or blocks starting at `from` up to `to` (inclusive).
    ChainRequest { from: u64, to: u64 },
    /// Initial handshake message.
    Handshake(Handshake),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_message_roundtrip() {
        let msg = WireMessage::TxBroadcast { tx: vec![1, 2, 3] };
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: WireMessage = bincode::deserialize(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }
}
