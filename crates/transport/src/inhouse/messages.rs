use std::convert::TryInto;
use std::fmt;

pub const MAGIC: &[u8; 4] = b"TBQH";
pub const VERSION: u8 = 1;
const CLIENT_HELLO: u8 = 0x01;
const SERVER_HELLO: u8 = 0x02;
const CLIENT_FINISH: u8 = 0x03;
const APPLICATION_DATA: u8 = 0x10;
const APPLICATION_ACK: u8 = 0x11;

pub const MAX_DATAGRAM: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageError {
    TooShort,
    InvalidMagic,
    InvalidVersion(u8),
    UnknownKind(u8),
    Truncated,
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageError::TooShort => f.write_str("packet too short"),
            MessageError::InvalidMagic => f.write_str("invalid magic bytes"),
            MessageError::InvalidVersion(v) => {
                write!(f, "unsupported protocol version {v}")
            }
            MessageError::UnknownKind(kind) => write!(f, "unknown message kind {kind}"),
            MessageError::Truncated => f.write_str("payload truncated"),
        }
    }
}

impl std::error::Error for MessageError {}

#[derive(Debug, PartialEq, Eq)]
pub enum Message {
    ClientHello {
        handshake: [u8; 16],
    },
    ServerHello {
        handshake: [u8; 16],
        fingerprint: [u8; 32],
        certificate: Vec<u8>,
    },
    ClientFinish {
        handshake: [u8; 16],
    },
    ApplicationData {
        handshake: [u8; 16],
        payload: Vec<u8>,
    },
    ApplicationAck {
        handshake: [u8; 16],
        payload: Vec<u8>,
    },
}

pub fn encode_client_hello(handshake: &[u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 1 + 1 + 16);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(CLIENT_HELLO);
    out.extend_from_slice(handshake);
    out
}

pub fn encode_server_hello(
    handshake: &[u8; 16],
    fingerprint: &[u8; 32],
    certificate: &[u8],
) -> Vec<u8> {
    let cert_len: u16 = certificate.len().try_into().unwrap_or(u16::MAX);
    let mut out = Vec::with_capacity(4 + 1 + 1 + 16 + 32 + 2 + certificate.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(SERVER_HELLO);
    out.extend_from_slice(handshake);
    out.extend_from_slice(fingerprint);
    out.extend_from_slice(&cert_len.to_be_bytes());
    out.extend_from_slice(certificate);
    out
}

pub fn encode_client_finish(handshake: &[u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 1 + 1 + 16);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(CLIENT_FINISH);
    out.extend_from_slice(handshake);
    out
}

pub fn encode_application_data(handshake: &[u8; 16], payload: &[u8]) -> Vec<u8> {
    let len: u16 = payload.len().try_into().unwrap_or(u16::MAX);
    let mut out = Vec::with_capacity(4 + 1 + 1 + 16 + 2 + payload.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(APPLICATION_DATA);
    out.extend_from_slice(handshake);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn encode_application_ack(handshake: &[u8; 16], payload: &[u8]) -> Vec<u8> {
    let len: u16 = payload.len().try_into().unwrap_or(u16::MAX);
    let mut out = Vec::with_capacity(4 + 1 + 1 + 16 + 2 + payload.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(APPLICATION_ACK);
    out.extend_from_slice(handshake);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn decode_message(input: &[u8]) -> Result<Message, MessageError> {
    if input.len() < 6 {
        return Err(MessageError::TooShort);
    }
    if &input[..4] != MAGIC {
        return Err(MessageError::InvalidMagic);
    }
    let version = input[4];
    if version != VERSION {
        return Err(MessageError::InvalidVersion(version));
    }
    let kind = input[5];
    let body = &input[6..];
    match kind {
        CLIENT_HELLO => {
            if body.len() < 16 {
                return Err(MessageError::Truncated);
            }
            let handshake = body[..16].try_into().expect("slice length");
            Ok(Message::ClientHello { handshake })
        }
        SERVER_HELLO => {
            if body.len() < 16 + 32 + 2 {
                return Err(MessageError::Truncated);
            }
            let handshake = body[..16].try_into().expect("slice length");
            let fingerprint = body[16..48].try_into().expect("slice length");
            let len = u16::from_be_bytes(body[48..50].try_into().expect("slice length")) as usize;
            if body.len() < 50 + len {
                return Err(MessageError::Truncated);
            }
            let certificate = body[50..50 + len].to_vec();
            Ok(Message::ServerHello {
                handshake,
                fingerprint,
                certificate,
            })
        }
        CLIENT_FINISH => {
            if body.len() < 16 {
                return Err(MessageError::Truncated);
            }
            let handshake = body[..16].try_into().expect("slice length");
            Ok(Message::ClientFinish { handshake })
        }
        APPLICATION_DATA | APPLICATION_ACK => {
            if body.len() < 16 + 2 {
                return Err(MessageError::Truncated);
            }
            let handshake = body[..16].try_into().expect("slice length");
            let len = u16::from_be_bytes(body[16..18].try_into().expect("slice length")) as usize;
            if body.len() < 18 + len {
                return Err(MessageError::Truncated);
            }
            let payload = body[18..18 + len].to_vec();
            if kind == APPLICATION_DATA {
                Ok(Message::ApplicationData { handshake, payload })
            } else {
                Ok(Message::ApplicationAck { handshake, payload })
            }
        }
        other => Err(MessageError::UnknownKind(other)),
    }
}
