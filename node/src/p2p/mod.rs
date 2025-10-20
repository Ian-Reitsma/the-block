pub mod handshake;
pub use handshake::*;
pub mod wire_binary;
pub use wire_binary::{decode as decode_wire_message, encode as encode_wire_message};

/// Messages exchanged between peers once a connection is established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireMessage {
    /// Broadcast a serialized transaction to peers.
    TxBroadcast { tx: Vec<u8> },
    /// Announce a newly mined block to peers.
    BlockAnnounce { block: Vec<u8> },
    /// Request headers or blocks starting at `from` up to `to` (inclusive).
    ChainRequest { from: u64, to: u64 },
    /// Initial handshake message.
    Handshake(Hello),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_message_roundtrip() {
        let msg = WireMessage::TxBroadcast { tx: vec![1, 2, 3] };
        let bytes = wire_binary::encode(&msg).expect("encode wire message");
        let decoded = wire_binary::decode(&bytes).expect("decode wire message");
        assert_eq!(msg, decoded);
    }
}
