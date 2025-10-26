//! Minimal WebSocket primitives built on top of the runtime TCP reactor.
//!
//! The implementation focuses on RFC 6455 server/client handshakes, frame
//! encoding/decoding (including masking and fragmented message reassembly) and
//! basic ping/pong management so higher layers can migrate away from
//! third-party stacks.

use crate::net::TcpStream;
use base64_fp::encode_standard;
use crypto_suite::hashing::sha1;
use crypto_suite::Error as CryptoError;
use rand::RngCore;
use std::future::Future;
use std::io::{self, Error, ErrorKind};
use std::pin::Pin;

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

type Result<T> = core::result::Result<T, CryptoError>;

pub type IoFuture<'a, T> = Pin<Box<dyn Future<Output = io::Result<T>> + Send + 'a>>;

/// Minimal async IO abstraction allowing WebSocket streams to operate over
/// first-party TCP as well as TLS transports surfaced by the HTTP server.
pub trait WebSocketIo: Send + 'static {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> IoFuture<'a, usize>;
    fn write<'a>(&'a mut self, buf: &'a [u8]) -> IoFuture<'a, usize>;
    fn flush(&mut self) -> IoFuture<'_, ()>;
    fn shutdown(&mut self) -> IoFuture<'_, ()>;
}

/// Convenience helpers mirroring `TcpStream`'s extension methods for any
/// [`WebSocketIo`] implementation.
pub trait WebSocketIoExt: WebSocketIo {
    fn read_exact<'a>(&'a mut self, buf: &'a mut [u8]) -> IoFuture<'a, ()> {
        Box::pin(async move {
            let mut offset = 0usize;
            while offset < buf.len() {
                let read = self.read(&mut buf[offset..]).await?;
                if read == 0 {
                    return Err(io::Error::new(
                        ErrorKind::UnexpectedEof,
                        "websocket transport reached eof",
                    ));
                }
                offset += read;
            }
            Ok(())
        })
    }

    fn write_all<'a>(&'a mut self, mut buf: &'a [u8]) -> IoFuture<'a, ()> {
        Box::pin(async move {
            while !buf.is_empty() {
                let written = self.write(buf).await?;
                if written == 0 {
                    return Err(io::Error::new(
                        ErrorKind::WriteZero,
                        "websocket transport failed to write remaining bytes",
                    ));
                }
                buf = &buf[written..];
            }
            Ok(())
        })
    }
}

impl<T> WebSocketIoExt for T where T: WebSocketIo + ?Sized {}

impl WebSocketIo for TcpStream {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> IoFuture<'a, usize> {
        Box::pin(async move { TcpStream::read(self, buf).await })
    }

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> IoFuture<'a, usize> {
        Box::pin(async move { TcpStream::write(self, buf).await })
    }

    fn flush(&mut self) -> IoFuture<'_, ()> {
        Box::pin(async move { TcpStream::flush(self).await })
    }

    fn shutdown(&mut self) -> IoFuture<'_, ()> {
        Box::pin(async move { TcpStream::shutdown(self).await })
    }
}

/// Generate the Sec-WebSocket-Accept header value for a given client key.
pub fn handshake_accept(key: &str) -> Result<String> {
    let mut data = key.as_bytes().to_vec();
    data.extend_from_slice(GUID.as_bytes());
    let digest = sha1::hash(&data)?;
    Ok(encode_standard(&digest))
}

/// Generate a random Sec-WebSocket-Key suitable for initiating a client
/// handshake.
pub fn handshake_key() -> String {
    let mut key = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut key);
    encode_standard(&key)
}

/// Additional headers that should be appended to the handshake response.
fn build_header_block(headers: &[(&str, &str)]) -> String {
    headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect()
}

/// Write a server-side handshake response to the socket.
pub async fn write_server_handshake<I>(
    stream: &mut I,
    key: &str,
    extra_headers: &[(&str, &str)],
) -> io::Result<()>
where
    I: WebSocketIo,
{
    let accept = handshake_accept(key).map_err(|err| io::Error::new(ErrorKind::Other, err))?;
    let mut response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n"
    );
    response.push_str(&build_header_block(extra_headers));
    response.push_str("\r\n");
    stream.write_all(response.as_bytes()).await
}

/// Read a client-side handshake response and validate the accept key.
pub async fn read_client_handshake<I>(stream: &mut I, expected_accept: &str) -> io::Result<()>
where
    I: WebSocketIo,
{
    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 64];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "websocket handshake ended prematurely",
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 8192 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "websocket handshake headers too large",
            ));
        }
    }

    let headers = std::str::from_utf8(&buf)
        .map_err(|_| Error::new(ErrorKind::InvalidData, "invalid utf8 in handshake"))?;
    if !headers.starts_with("HTTP/1.1 101") {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "server declined websocket upgrade",
        ));
    }

    let mut found = false;
    for line in headers.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("sec-websocket-accept")
                && value.trim() == expected_accept
            {
                found = true;
                break;
            }
        }
    }
    if !found {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "websocket accept key mismatch",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Role {
    Server,
    Client,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum OpCode {
    Continuation,
    Text,
    Binary,
    Close,
    Ping,
    Pong,
}

impl OpCode {
    fn from_byte(byte: u8) -> io::Result<Self> {
        match byte {
            0x0 => Ok(Self::Continuation),
            0x1 => Ok(Self::Text),
            0x2 => Ok(Self::Binary),
            0x8 => Ok(Self::Close),
            0x9 => Ok(Self::Ping),
            0xA => Ok(Self::Pong),
            _ => Err(Error::new(ErrorKind::InvalidData, "unsupported opcode")),
        }
    }

    fn to_byte(self) -> u8 {
        match self {
            Self::Continuation => 0x0,
            Self::Text => 0x1,
            Self::Binary => 0x2,
            Self::Close => 0x8,
            Self::Ping => 0x9,
            Self::Pong => 0xA,
        }
    }
}

#[derive(Debug)]
struct Frame {
    opcode: OpCode,
    fin: bool,
    payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseFrame {
    pub code: u16,
    pub reason: String,
}

/// WebSocket messages surfaced to callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<CloseFrame>),
}

struct Fragment {
    opcode: OpCode,
    data: Vec<u8>,
}

/// Bidirectional WebSocket stream.
pub struct WebSocketStream {
    stream: Box<dyn WebSocketIo>,
    role: Role,
    fragment: Option<Fragment>,
    closing: bool,
    closed: bool,
}

impl WebSocketStream {
    fn new(stream: Box<dyn WebSocketIo>, role: Role) -> Self {
        Self {
            stream,
            role,
            fragment: None,
            closing: false,
            closed: false,
        }
    }

    async fn read_frame(&mut self) -> io::Result<Frame> {
        let mut header = [0u8; 2];
        self.stream.read_exact(&mut header).await?;
        let fin = header[0] & 0x80 != 0;
        let opcode = OpCode::from_byte(header[0] & 0x0F)?;
        let masked = header[1] & 0x80 != 0;
        let mut len = (header[1] & 0x7F) as u64;
        if len == 126 {
            let mut extended = [0u8; 2];
            self.stream.read_exact(&mut extended).await?;
            len = u16::from_be_bytes(extended) as u64;
        } else if len == 127 {
            let mut extended = [0u8; 8];
            self.stream.read_exact(&mut extended).await?;
            len = u64::from_be_bytes(extended);
        }

        if len > (1 << 31) {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "websocket frame exceeds 2 GiB limit",
            ));
        }

        let mut mask = [0u8; 4];
        if masked {
            self.stream.read_exact(&mut mask).await?;
        }

        let mut payload = vec![0u8; len as usize];
        if len > 0 {
            self.stream.read_exact(&mut payload).await?;
            if masked {
                for (i, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[i % 4];
                }
            }
        }

        if matches!(opcode, OpCode::Ping | OpCode::Pong | OpCode::Close) && payload.len() > 125 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "control frame payload too large",
            ));
        }

        Ok(Frame {
            opcode,
            fin,
            payload,
        })
    }

    async fn write_frame(&mut self, opcode: OpCode, fin: bool, payload: &[u8]) -> io::Result<()> {
        let mut header = Vec::with_capacity(10);
        header.push((if fin { 0x80 } else { 0x00 }) | opcode.to_byte());
        let mask_payload = matches!(self.role, Role::Client);
        let len_byte = payload.len() as u64;
        if len_byte < 126 {
            header.push((if mask_payload { 0x80 } else { 0x00 }) | (len_byte as u8));
        } else if len_byte <= u16::MAX as u64 {
            header.push(if mask_payload { 0xFE } else { 126 });
            header.extend_from_slice(&(len_byte as u16).to_be_bytes());
        } else {
            header.push(if mask_payload { 0xFF } else { 127 });
            header.extend_from_slice(&len_byte.to_be_bytes());
        }

        let mut mask = [0u8; 4];
        if mask_payload {
            rand::thread_rng().fill_bytes(&mut mask);
            header.extend_from_slice(&mask);
        }

        self.stream.write_all(&header).await?;
        if payload.is_empty() {
            return Ok(());
        }

        if mask_payload {
            let mut masked_payload = payload.to_vec();
            for (i, byte) in masked_payload.iter_mut().enumerate() {
                *byte ^= mask[i % 4];
            }
            self.stream.write_all(&masked_payload).await?
        } else {
            self.stream.write_all(payload).await?;
        }
        Ok(())
    }

    async fn ensure_close_frame(&mut self, payload: &[u8]) -> io::Result<Option<CloseFrame>> {
        if payload.is_empty() {
            return Ok(None);
        }
        if payload.len() < 2 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "close frame payload must contain status code",
            ));
        }
        let code = u16::from_be_bytes([payload[0], payload[1]]);
        let reason_bytes = &payload[2..];
        let reason = std::str::from_utf8(reason_bytes)
            .map_err(|_| Error::new(ErrorKind::InvalidData, "close reason is not utf8"))?
            .to_string();
        Ok(Some(CloseFrame { code, reason }))
    }

    async fn handle_fragmented(&mut self, frame: Frame) -> io::Result<Option<Message>> {
        match frame.opcode {
            OpCode::Continuation => {
                let Some(fragment) = &mut self.fragment else {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        "continuation without initial fragment",
                    ));
                };
                fragment.data.extend_from_slice(&frame.payload);
                if frame.fin {
                    debug_assert!(self.fragment.is_some(), "fragment must exist on finalize");
                    let fragment = self
                        .fragment
                        .take()
                        .unwrap_or_else(|| unreachable!("fragment must exist on finalize"));
                    return Self::finalize_fragment(fragment);
                }
                Ok(None)
            }
            OpCode::Text | OpCode::Binary => {
                if self.fragment.is_some() {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        "new data frame while fragmented message pending",
                    ));
                }
                if frame.fin {
                    return Self::finalize_fragment(Fragment {
                        opcode: frame.opcode,
                        data: frame.payload,
                    });
                }
                self.fragment = Some(Fragment {
                    opcode: frame.opcode,
                    data: frame.payload,
                });
                Ok(None)
            }
            _ => Err(Error::new(
                ErrorKind::InvalidData,
                "control frames cannot be fragmented",
            )),
        }
    }

    fn finalize_fragment(fragment: Fragment) -> io::Result<Option<Message>> {
        match fragment.opcode {
            OpCode::Text => {
                let text = String::from_utf8(fragment.data)
                    .map_err(|_| Error::new(ErrorKind::InvalidData, "invalid utf8 payload"))?;
                Ok(Some(Message::Text(text)))
            }
            OpCode::Binary => Ok(Some(Message::Binary(fragment.data))),
            _ => Err(Error::new(
                ErrorKind::InvalidData,
                "unexpected opcode for fragmented message",
            )),
        }
    }

    /// Receive the next message from the stream.
    pub async fn recv(&mut self) -> io::Result<Option<Message>> {
        if self.closed {
            return Ok(None);
        }

        loop {
            let frame = self.read_frame().await?;
            match frame.opcode {
                OpCode::Continuation | OpCode::Text | OpCode::Binary => {
                    if let Some(msg) = self.handle_fragmented(frame).await? {
                        return Ok(Some(msg));
                    }
                }
                OpCode::Ping => {
                    self.write_frame(OpCode::Pong, true, &frame.payload).await?;
                    return Ok(Some(Message::Ping(frame.payload)));
                }
                OpCode::Pong => return Ok(Some(Message::Pong(frame.payload))),
                OpCode::Close => {
                    if !self.closing {
                        self.write_frame(OpCode::Close, true, &frame.payload)
                            .await?;
                    }
                    self.closed = true;
                    let close = self.ensure_close_frame(&frame.payload).await?;
                    return Ok(Some(Message::Close(close)));
                }
            }
        }
    }

    /// Send a WebSocket message.
    pub async fn send(&mut self, msg: Message) -> io::Result<()> {
        if self.closed {
            return Err(Error::new(
                ErrorKind::NotConnected,
                "websocket connection already closed",
            ));
        }

        match msg {
            Message::Text(text) => self.write_frame(OpCode::Text, true, text.as_bytes()).await,
            Message::Binary(data) => self.write_frame(OpCode::Binary, true, &data).await,
            Message::Ping(payload) => self.write_frame(OpCode::Ping, true, &payload).await,
            Message::Pong(payload) => self.write_frame(OpCode::Pong, true, &payload).await,
            Message::Close(frame) => {
                let mut payload = Vec::new();
                if let Some(frame) = frame {
                    payload.extend_from_slice(&frame.code.to_be_bytes());
                    payload.extend_from_slice(frame.reason.as_bytes());
                }
                self.closing = true;
                self.write_frame(OpCode::Close, true, &payload).await?;
                self.closed = true;
                Ok(())
            }
        }
    }

    /// Perform a graceful close handshake without a custom close frame.
    pub async fn close(&mut self) -> io::Result<()> {
        self.send(Message::Close(None)).await
    }

    /// Access to the underlying transport when required by higher layers.
    pub fn into_inner(self) -> Box<dyn WebSocketIo> {
        self.stream
    }
}

/// Server side WebSocket stream wrapper.
pub struct ServerStream(WebSocketStream);

impl ServerStream {
    pub fn new(stream: TcpStream) -> Self {
        Self::from_io(stream)
    }

    pub fn from_io<I>(stream: I) -> Self
    where
        I: WebSocketIo,
    {
        Self(WebSocketStream::new(Box::new(stream), Role::Server))
    }

    pub async fn recv(&mut self) -> io::Result<Option<Message>> {
        self.0.recv().await
    }

    pub async fn send(&mut self, msg: Message) -> io::Result<()> {
        self.0.send(msg).await
    }

    pub async fn close(&mut self) -> io::Result<()> {
        self.0.close().await
    }

    pub fn into_inner(self) -> Box<dyn WebSocketIo> {
        self.0.into_inner()
    }
}

/// Client side WebSocket stream wrapper.
pub struct ClientStream(WebSocketStream);

impl ClientStream {
    pub fn new(stream: TcpStream) -> Self {
        Self::from_io(stream)
    }

    pub fn from_io<I>(stream: I) -> Self
    where
        I: WebSocketIo,
    {
        Self(WebSocketStream::new(Box::new(stream), Role::Client))
    }

    pub async fn recv(&mut self) -> io::Result<Option<Message>> {
        self.0.recv().await
    }

    pub async fn send(&mut self, msg: Message) -> io::Result<()> {
        self.0.send(msg).await
    }

    pub async fn close(&mut self) -> io::Result<()> {
        self.0.close().await
    }

    pub fn into_inner(self) -> Box<dyn WebSocketIo> {
        self.0.into_inner()
    }
}
