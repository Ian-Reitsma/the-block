use std::convert::TryFrom;
use std::fmt;
use std::net::SocketAddr;

use concurrency::Bytes;
use foundation_serialization::binary_cursor::{Reader, Writer};

use crate::p2p::handshake::{Hello, Transport};
use crate::p2p::WireMessage;
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};

/// Result alias for encoding helpers.
pub type EncodeResult<T> = Result<T, EncodeError>;

/// Error returned when encoding fails.
#[derive(Debug)]
pub enum EncodeError {
    /// Collection length exceeded `u64::MAX`.
    LengthOverflow(&'static str),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::LengthOverflow(field) => {
                write!(f, "{field} length exceeds u64::MAX")
            }
        }
    }
}

impl std::error::Error for EncodeError {}

/// Encode a [`WireMessage`] using the legacy binary representation.
pub fn encode(message: &WireMessage) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::new();
    write_wire_message(&mut writer, message)?;
    Ok(writer.finish())
}

/// Decode a [`WireMessage`] from the legacy binary representation.
pub fn decode(bytes: &[u8]) -> binary_struct::Result<WireMessage> {
    let mut reader = Reader::new(bytes);
    let message = read_wire_message(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(message)
}

fn write_wire_message(writer: &mut Writer, message: &WireMessage) -> EncodeResult<()> {
    match message {
        WireMessage::TxBroadcast { tx } => {
            writer.write_u32(0);
            writer.write_u64(1);
            writer.write_string("tx");
            write_byte_slice(writer, tx);
        }
        WireMessage::BlockAnnounce { block } => {
            writer.write_u32(1);
            writer.write_u64(1);
            writer.write_string("block");
            write_byte_slice(writer, block);
        }
        WireMessage::ChainRequest { from, to } => {
            writer.write_u32(2);
            writer.write_u64(2);
            writer.write_string("from");
            writer.write_u64(*from);
            writer.write_string("to");
            writer.write_u64(*to);
        }
        WireMessage::Handshake(hello) => {
            writer.write_u32(3);
            write_hello(writer, hello)?;
        }
    }
    Ok(())
}

fn read_wire_message(reader: &mut Reader<'_>) -> binary_struct::Result<WireMessage> {
    let variant = reader.read_u32()?;
    match variant {
        0 => {
            let field_count = reader.read_u64()?;
            if field_count != 1 {
                return Err(DecodeError::InvalidFieldCount {
                    expected: 1,
                    actual: field_count,
                });
            }
            let key = reader.read_string()?;
            if key != "tx" {
                return Err(DecodeError::UnknownField(key));
            }
            let tx = reader.read_bytes()?;
            Ok(WireMessage::TxBroadcast { tx })
        }
        1 => {
            let field_count = reader.read_u64()?;
            if field_count != 1 {
                return Err(DecodeError::InvalidFieldCount {
                    expected: 1,
                    actual: field_count,
                });
            }
            let key = reader.read_string()?;
            if key != "block" {
                return Err(DecodeError::UnknownField(key));
            }
            let block = reader.read_bytes()?;
            Ok(WireMessage::BlockAnnounce { block })
        }
        2 => {
            let field_count = reader.read_u64()?;
            if field_count != 2 {
                return Err(DecodeError::InvalidFieldCount {
                    expected: 2,
                    actual: field_count,
                });
            }
            let mut from = None;
            let mut to = None;
            for _ in 0..field_count {
                let key = reader.read_string()?;
                match key.as_str() {
                    "from" => {
                        from = Some(reader.read_u64()?);
                    }
                    "to" => {
                        to = Some(reader.read_u64()?);
                    }
                    other => return Err(DecodeError::UnknownField(other.to_owned())),
                }
            }
            Ok(WireMessage::ChainRequest {
                from: from.ok_or(DecodeError::MissingField("from"))?,
                to: to.ok_or(DecodeError::MissingField("to"))?,
            })
        }
        3 => {
            let hello = read_hello(reader)?;
            Ok(WireMessage::Handshake(hello))
        }
        other => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "WireMessage",
            value: other,
        }),
    }
}

fn write_hello(writer: &mut Writer, hello: &Hello) -> EncodeResult<()> {
    let fingerprint_prev_len = to_u64(
        hello.quic_fingerprint_previous.len(),
        "quic_fingerprint_previous",
    )?;
    let capabilities_len = to_u64(hello.quic_capabilities.len(), "quic_capabilities")?;

    writer.write_struct(|s| {
        s.field_with("network_id", |w| {
            write_array_u8(w, hello.network_id.as_ref());
        });
        s.field_with("proto_version", |w| w.write_u16(hello.proto_version));
        s.field_with("feature_bits", |w| w.write_u32(hello.feature_bits));
        s.field_string("agent", &hello.agent);
        s.field_with("nonce", |w| w.write_u64(hello.nonce));
        s.field_with("transport", |w| write_transport(w, hello.transport));
        s.field_with("quic_addr", |w| {
            write_option_socket_addr(w, hello.quic_addr.as_ref())
        });
        s.field_with("quic_cert", |w| {
            write_option_bytes(w, hello.quic_cert.as_ref())
        });
        s.field_with("quic_fingerprint", |w| {
            write_option_bytes(w, hello.quic_fingerprint.as_ref());
        });
        s.field_with("quic_fingerprint_previous", |w| {
            w.write_u64(fingerprint_prev_len);
            for value in &hello.quic_fingerprint_previous {
                w.write_bytes(value.as_ref());
            }
        });
        s.field_with("quic_provider", |w| {
            write_option_str(w, hello.quic_provider.as_deref())
        });
        s.field_with("quic_capabilities", |w| {
            w.write_u64(capabilities_len);
            for cap in &hello.quic_capabilities {
                w.write_string(cap);
            }
        });
    });
    Ok(())
}

fn read_hello(reader: &mut Reader<'_>) -> binary_struct::Result<Hello> {
    read_hello_struct(reader)
}

fn read_hello_struct(reader: &mut Reader<'_>) -> binary_struct::Result<Hello> {
    let mut network_id = None;
    let mut proto_version = None;
    let mut feature_bits = None;
    let mut agent = None;
    let mut nonce = None;
    let mut transport = None;
    let mut quic_addr = None;
    let mut quic_addr_seen = false;
    let mut quic_cert = None;
    let mut quic_cert_seen = false;
    let mut quic_fingerprint = None;
    let mut quic_fingerprint_seen = false;
    let mut quic_fingerprint_previous = None;
    let mut quic_provider = None;
    let mut quic_provider_seen = false;
    let mut quic_capabilities = None;

    decode_struct(reader, Some(12), |key, reader| match key {
        "network_id" => assign_once(&mut network_id, read_array_u8::<4>(reader)?, "network_id"),
        "proto_version" => assign_once(&mut proto_version, reader.read_u16()?, "proto_version"),
        "feature_bits" => assign_once(&mut feature_bits, reader.read_u32()?, "feature_bits"),
        "agent" => assign_once(&mut agent, reader.read_string()?, "agent"),
        "nonce" => assign_once(&mut nonce, reader.read_u64()?, "nonce"),
        "transport" => assign_once(&mut transport, read_transport(reader)?, "transport"),
        "quic_addr" => assign_optional_field(
            &mut quic_addr_seen,
            &mut quic_addr,
            reader.read_option_with(|reader| read_socket_addr(reader, "quic_addr"))?,
            "quic_addr",
        ),
        "quic_cert" => assign_optional_field(
            &mut quic_cert_seen,
            &mut quic_cert,
            reader.read_option_with(|reader| reader.read_bytes().map(Bytes::from))?,
            "quic_cert",
        ),
        "quic_fingerprint" => assign_optional_field(
            &mut quic_fingerprint_seen,
            &mut quic_fingerprint,
            reader.read_option_with(|reader| reader.read_bytes().map(Bytes::from))?,
            "quic_fingerprint",
        ),
        "quic_fingerprint_previous" => assign_once(
            &mut quic_fingerprint_previous,
            reader.read_vec_with(|reader| reader.read_bytes().map(Bytes::from))?,
            "quic_fingerprint_previous",
        ),
        "quic_provider" => assign_optional_field(
            &mut quic_provider_seen,
            &mut quic_provider,
            reader.read_option_with(|reader| reader.read_string())?,
            "quic_provider",
        ),
        "quic_capabilities" => assign_once(
            &mut quic_capabilities,
            reader.read_vec_with(|reader| reader.read_string())?,
            "quic_capabilities",
        ),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(Hello {
        network_id: network_id.ok_or(DecodeError::MissingField("network_id"))?,
        proto_version: proto_version.ok_or(DecodeError::MissingField("proto_version"))?,
        feature_bits: feature_bits.ok_or(DecodeError::MissingField("feature_bits"))?,
        agent: agent.ok_or(DecodeError::MissingField("agent"))?,
        nonce: nonce.ok_or(DecodeError::MissingField("nonce"))?,
        transport: transport.ok_or(DecodeError::MissingField("transport"))?,
        quic_addr,
        quic_cert,
        quic_fingerprint,
        quic_fingerprint_previous: quic_fingerprint_previous
            .ok_or(DecodeError::MissingField("quic_fingerprint_previous"))?,
        quic_provider,
        quic_capabilities: quic_capabilities
            .ok_or(DecodeError::MissingField("quic_capabilities"))?,
    })
}

fn write_transport(writer: &mut Writer, transport: Transport) {
    match transport {
        Transport::Tcp => writer.write_u32(0),
        Transport::Quic => writer.write_u32(1),
    }
}

fn read_transport(reader: &mut Reader<'_>) -> Result<Transport, DecodeError> {
    match reader.read_u32()? {
        0 => Ok(Transport::Tcp),
        1 => Ok(Transport::Quic),
        value => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "Transport",
            value,
        }),
    }
}

fn write_socket_addr(writer: &mut Writer, addr: &SocketAddr) {
    writer.write_string(&addr.to_string());
}

fn read_socket_addr(
    reader: &mut Reader<'_>,
    field: &'static str,
) -> Result<SocketAddr, DecodeError> {
    let value = reader.read_string()?;
    value.parse().map_err(
        |err: std::net::AddrParseError| DecodeError::InvalidFieldValue {
            field,
            reason: err.to_string(),
        },
    )
}

fn read_array_u8<const N: usize>(reader: &mut Reader<'_>) -> Result<[u8; N], DecodeError> {
    let bytes = reader.read_vec_with(|reader| reader.read_u8())?;
    if bytes.len() != N {
        return Err(DecodeError::InvalidFieldCount {
            expected: N as u64,
            actual: bytes.len() as u64,
        });
    }
    let mut arr = [0u8; N];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn write_array_u8(writer: &mut Writer, bytes: &[u8]) {
    write_byte_slice(writer, bytes);
}

fn write_byte_slice(writer: &mut Writer, bytes: &[u8]) {
    writer.write_bytes(bytes);
}

fn write_option_socket_addr(writer: &mut Writer, value: Option<&SocketAddr>) {
    match value {
        Some(addr) => {
            writer.write_bool(true);
            write_socket_addr(writer, addr);
        }
        None => writer.write_bool(false),
    }
}

fn assign_optional_field<T>(
    seen: &mut bool,
    slot: &mut Option<T>,
    value: Option<T>,
    name: &'static str,
) -> binary_struct::Result<()> {
    if *seen {
        return Err(DecodeError::DuplicateField(name));
    }
    *seen = true;
    *slot = value;
    Ok(())
}

fn write_option_bytes(writer: &mut Writer, value: Option<&Bytes>) {
    match value {
        Some(bytes) => {
            writer.write_bool(true);
            writer.write_bytes(bytes.as_ref());
        }
        None => writer.write_bool(false),
    }
}

fn write_option_str(writer: &mut Writer, value: Option<&str>) {
    match value {
        Some(text) => {
            writer.write_bool(true);
            writer.write_string(text);
        }
        None => writer.write_bool(false),
    }
}

fn to_u64(len: usize, field: &'static str) -> EncodeResult<u64> {
    u64::try_from(len).map_err(|_| EncodeError::LengthOverflow(field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::handshake::{Hello, Transport};
    use crate::util::binary_codec;
    use foundation_serialization::{Deserialize, Serialize};
    use std::net::SocketAddr;

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
    enum LegacyWireMessage {
        TxBroadcast { tx: Vec<u8> },
        BlockAnnounce { block: Vec<u8> },
        ChainRequest { from: u64, to: u64 },
        Handshake(Hello),
    }

    impl From<&WireMessage> for LegacyWireMessage {
        fn from(msg: &WireMessage) -> Self {
            match msg {
                WireMessage::TxBroadcast { tx } => Self::TxBroadcast { tx: tx.clone() },
                WireMessage::BlockAnnounce { block } => Self::BlockAnnounce {
                    block: block.clone(),
                },
                WireMessage::ChainRequest { from, to } => Self::ChainRequest {
                    from: *from,
                    to: *to,
                },
                WireMessage::Handshake(hello) => Self::Handshake(hello.clone()),
            }
        }
    }

    impl From<LegacyWireMessage> for WireMessage {
        fn from(msg: LegacyWireMessage) -> Self {
            match msg {
                LegacyWireMessage::TxBroadcast { tx } => WireMessage::TxBroadcast { tx },
                LegacyWireMessage::BlockAnnounce { block } => WireMessage::BlockAnnounce { block },
                LegacyWireMessage::ChainRequest { from, to } => {
                    WireMessage::ChainRequest { from, to }
                }
                LegacyWireMessage::Handshake(hello) => WireMessage::Handshake(hello),
            }
        }
    }

    fn assert_matches_legacy(message: WireMessage) {
        let expected =
            binary_codec::serialize(&LegacyWireMessage::from(&message)).expect("legacy encode");
        let encoded = encode(&message).expect("encode");
        assert_eq!(expected, encoded);

        let decoded = decode(&encoded).expect("decode");
        assert_eq!(message, decoded);

        let legacy_decoded =
            binary_codec::deserialize::<LegacyWireMessage>(&encoded).expect("legacy decode");
        assert_eq!(message, WireMessage::from(legacy_decoded));
    }

    #[test]
    fn wire_message_matches_legacy() {
        let hello = Hello {
            network_id: [1, 2, 3, 4],
            proto_version: 7,
            feature_bits: 0b1010,
            agent: "blockd/1.0".to_string(),
            nonce: 42,
            transport: Transport::Tcp,
            quic_addr: Some(SocketAddr::from(([127, 0, 0, 1], 8080))),
            quic_cert: Some(Bytes::from(vec![1, 2, 3])),
            quic_fingerprint: None,
            quic_fingerprint_previous: vec![Bytes::from(vec![9, 9])],
            quic_provider: Some("provider".to_string()),
            quic_capabilities: vec!["cap".to_string()],
        };
        assert_matches_legacy(WireMessage::Handshake(hello.clone()));

        let mut hello_without_optional = hello;
        hello_without_optional.quic_addr = None;
        hello_without_optional.quic_cert = None;
        hello_without_optional.quic_provider = None;
        hello_without_optional.quic_fingerprint = Some(Bytes::from(vec![7, 7, 7]));
        assert_matches_legacy(WireMessage::Handshake(hello_without_optional));
    }

    #[test]
    fn tx_broadcast_matches_legacy() {
        assert_matches_legacy(WireMessage::TxBroadcast {
            tx: vec![0xAA, 0xBB, 0xCC],
        });
    }

    #[test]
    fn block_announce_matches_legacy() {
        assert_matches_legacy(WireMessage::BlockAnnounce {
            block: vec![0x10, 0x20, 0x30, 0x40],
        });
    }

    #[test]
    fn chain_request_matches_legacy() {
        assert_matches_legacy(WireMessage::ChainRequest { from: 5, to: 42 });
    }
}
