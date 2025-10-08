use diagnostics::{debug, info};
use foundation_serialization::de::DeserializeOwned;
use foundation_serialization::{Error as SerializationError, Serialize, json};
use runtime::net::{TcpListener, TcpStream};
use runtime::ws::{self, ServerStream};
use runtime::{spawn, timeout};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io::{self, ErrorKind};
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

pub mod blocking;
pub mod client;
pub mod filters;
pub mod jsonrpc;
pub mod metrics;
mod tls;
use crate::tls::{AES_BLOCK, MAC_LEN};
pub mod uri;
pub use blocking::{BlockingClient, BlockingRequestBuilder};
pub use client::{Client as HttpClient, ClientConfig, ClientError, ClientResponse};
pub use jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcRouter};
pub use uri::{Uri, UriError, form_urlencoded, join_path};

/// Asynchronous IO abstraction allowing the HTTP server to operate over raw
/// TCP streams as well as TLS sessions that decrypt into an in-memory
/// plaintext transport.
pub trait ConnectionIo: Send + 'static {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> IoFuture<'a, usize>;
    fn write<'a>(&'a mut self, buf: &'a [u8]) -> IoFuture<'a, usize>;
    fn flush<'a>(&'a mut self) -> IoFuture<'a, ()>;
    fn shutdown<'a>(&'a mut self) -> IoFuture<'a, ()>;
}

pub trait UpgradeIo: ConnectionIo + Sized {
    type WebSocket: ws::WebSocketIo;
    fn supports_websocket(&self) -> bool;
    fn into_websocket(self) -> Result<Self::WebSocket, HttpError>;
}

/// Minimal buffering layer used by the HTTP parser. The implementation mirrors
/// `runtime::io::BufferedTcpStream` but operates on any transport satisfying the
/// [`ConnectionIo`] contract.
struct BufferedStream<S> {
    inner: S,
    buffer: Vec<u8>,
    consumed: usize,
}

impl<S> BufferedStream<S>
where
    S: ConnectionIo,
{
    fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: Vec::with_capacity(1024),
            consumed: 0,
        }
    }

    async fn read_line(&mut self, line: &mut String) -> io::Result<usize> {
        let initial_len = line.len();
        loop {
            if let Some(pos) = self.available().iter().position(|&b| b == b'\n') {
                let end = pos + 1;
                {
                    let available = self.available();
                    self.push_chunk(line, &available[..end])?;
                }
                self.consume(end);
                return Ok(line.len() - initial_len);
            }

            let mut temp = [0u8; 1024];
            let read = self.inner.read(&mut temp).await?;
            if read == 0 {
                if self.available().is_empty() {
                    return Ok(0);
                }
                let consumed = {
                    let available = self.available();
                    self.push_chunk(line, available)?;
                    available.len()
                };
                self.consume(consumed);
                return Ok(line.len() - initial_len);
            }
            self.buffer.extend_from_slice(&temp[..read]);
        }
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let mut offset = 0usize;
        if !self.available().is_empty() {
            let to_copy = {
                let available = self.available();
                let to_copy = buf.len().min(available.len());
                buf[..to_copy].copy_from_slice(&available[..to_copy]);
                to_copy
            };
            self.consume(to_copy);
            offset += to_copy;
        }
        while offset < buf.len() {
            let read = self.inner.read(&mut buf[offset..]).await?;
            if read == 0 {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "connection closed before filling buffer",
                ));
            }
            offset += read;
        }
        Ok(())
    }

    async fn write_all(&mut self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            let written = self.inner.write(buf).await?;
            if written == 0 {
                return Err(io::Error::new(
                    ErrorKind::WriteZero,
                    "connection failed to write remaining bytes",
                ));
            }
            buf = &buf[written..];
        }
        self.inner.flush().await
    }

    async fn shutdown(&mut self) -> io::Result<()> {
        self.inner.shutdown().await
    }

    fn into_inner(self) -> S {
        self.inner
    }

    fn inner_ref(&self) -> &S {
        &self.inner
    }

    fn available(&self) -> &[u8] {
        &self.buffer[self.consumed..]
    }

    fn consume(&mut self, amount: usize) {
        debug_assert!(amount <= self.available().len());
        self.consumed += amount;
        self.recycle_buffer();
    }

    fn recycle_buffer(&mut self) {
        if self.consumed == 0 {
            return;
        }
        if self.consumed >= self.buffer.len() {
            self.buffer.clear();
            self.consumed = 0;
            return;
        }
        if self.consumed > self.buffer.len() / 2 || self.buffer.len() > 4096 {
            let remaining = self.buffer.len() - self.consumed;
            self.buffer.copy_within(self.consumed.., 0);
            self.buffer.truncate(remaining);
            self.consumed = 0;
        }
    }

    fn push_chunk(&self, line: &mut String, chunk: &[u8]) -> io::Result<()> {
        match std::str::from_utf8(chunk) {
            Ok(part) => {
                line.push_str(part);
                Ok(())
            }
            Err(err) => Err(io::Error::new(ErrorKind::InvalidData, err)),
        }
    }
}

impl ConnectionIo for TcpStream {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> IoFuture<'a, usize> {
        Box::pin(async move { TcpStream::read(self, buf).await })
    }

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> IoFuture<'a, usize> {
        Box::pin(async move { TcpStream::write(self, buf).await })
    }

    fn flush<'a>(&'a mut self) -> IoFuture<'a, ()> {
        Box::pin(async move { TcpStream::flush(self).await })
    }

    fn shutdown<'a>(&'a mut self) -> IoFuture<'a, ()> {
        Box::pin(async move { TcpStream::shutdown(self).await })
    }
}

impl UpgradeIo for TcpStream {
    type WebSocket = TcpStream;

    fn supports_websocket(&self) -> bool {
        true
    }

    fn into_websocket(self) -> Result<Self::WebSocket, HttpError> {
        Ok(self)
    }
}

type IoFuture<'a, T> = Pin<Box<dyn Future<Output = io::Result<T>> + Send + 'a>>;

/// HTTP method enumeration supporting the subset of verbs required by the node
/// and metrics aggregator stacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Head,
    Patch,
    Options,
}

impl Method {
    fn from_bytes(bytes: &[u8]) -> Result<Self, HttpError> {
        match bytes {
            b"GET" => Ok(Method::Get),
            b"POST" => Ok(Method::Post),
            b"PUT" => Ok(Method::Put),
            b"DELETE" => Ok(Method::Delete),
            b"HEAD" => Ok(Method::Head),
            b"PATCH" => Ok(Method::Patch),
            b"OPTIONS" => Ok(Method::Options),
            _ => Err(HttpError::UnsupportedMethod),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Patch => "PATCH",
            Method::Options => "OPTIONS",
        }
    }
}

/// Minimal HTTP status code representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusCode(pub u16);

impl StatusCode {
    pub const OK: StatusCode = StatusCode(200);
    pub const CREATED: StatusCode = StatusCode(201);
    pub const ACCEPTED: StatusCode = StatusCode(202);
    pub const NO_CONTENT: StatusCode = StatusCode(204);
    pub const SWITCHING_PROTOCOLS: StatusCode = StatusCode(101);
    pub const BAD_REQUEST: StatusCode = StatusCode(400);
    pub const UNAUTHORIZED: StatusCode = StatusCode(401);
    pub const FORBIDDEN: StatusCode = StatusCode(403);
    pub const NOT_FOUND: StatusCode = StatusCode(404);
    pub const METHOD_NOT_ALLOWED: StatusCode = StatusCode(405);
    pub const CONFLICT: StatusCode = StatusCode(409);
    pub const TOO_MANY_REQUESTS: StatusCode = StatusCode(429);
    pub const PAYLOAD_TOO_LARGE: StatusCode = StatusCode(413);
    pub const UNSUPPORTED_MEDIA_TYPE: StatusCode = StatusCode(415);
    pub const INTERNAL_SERVER_ERROR: StatusCode = StatusCode(500);
    pub const SERVICE_UNAVAILABLE: StatusCode = StatusCode(503);

    fn reason(self) -> &'static str {
        match self.0 {
            200 => "OK",
            201 => "Created",
            202 => "Accepted",
            204 => "No Content",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            409 => "Conflict",
            429 => "Too Many Requests",
            413 => "Payload Too Large",
            415 => "Unsupported Media Type",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "Unknown",
        }
    }

    /// Returns the numeric representation of the status code.
    pub fn as_u16(self) -> u16 {
        self.0
    }

    /// Returns true when the status represents a successful response.
    pub fn is_success(self) -> bool {
        (200..300).contains(&self.0)
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Server level configuration used for request handling.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub request_timeout: Duration,
    pub keep_alive: Duration,
    pub max_body_bytes: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(10),
            keep_alive: Duration::from_secs(90),
            max_body_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone)]
pub struct ServerTlsConfig {
    inner: Arc<ServerTlsConfigInner>,
}

struct ServerTlsConfigInner {
    identity: tls::ServerIdentity,
    client_auth: tls::ClientAuthPolicy,
}

impl ServerTlsConfig {
    fn new(identity: tls::ServerIdentity, client_auth: tls::ClientAuthPolicy) -> Self {
        Self {
            inner: Arc::new(ServerTlsConfigInner {
                identity,
                client_auth,
            }),
        }
    }

    pub fn from_identity_files(
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> io::Result<Self> {
        let identity = tls::ServerIdentity::from_files(cert_path, key_path)?;
        Ok(Self::new(identity, tls::ClientAuthPolicy::None))
    }

    pub fn from_pem_files(
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> io::Result<Self> {
        Self::from_identity_files(cert_path, key_path)
    }

    pub fn from_identity_files_with_client_auth(
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
        registry_path: impl AsRef<Path>,
    ) -> io::Result<Self> {
        let identity = tls::ServerIdentity::from_files(cert_path, key_path)?;
        let registry = tls::ClientRegistry::from_path(registry_path)?;
        Ok(Self::new(
            identity,
            tls::ClientAuthPolicy::Required(registry),
        ))
    }

    pub fn from_pem_files_with_client_auth(
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
        registry_path: impl AsRef<Path>,
    ) -> io::Result<Self> {
        Self::from_identity_files_with_client_auth(cert_path, key_path, registry_path)
    }

    pub fn from_identity_files_with_optional_client_auth(
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
        registry_path: impl AsRef<Path>,
    ) -> io::Result<Self> {
        let identity = tls::ServerIdentity::from_files(cert_path, key_path)?;
        let registry = tls::ClientRegistry::from_path(registry_path)?;
        Ok(Self::new(
            identity,
            tls::ClientAuthPolicy::Optional(registry),
        ))
    }

    pub fn from_pem_files_with_optional_client_auth(
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
        registry_path: impl AsRef<Path>,
    ) -> io::Result<Self> {
        Self::from_identity_files_with_optional_client_auth(cert_path, key_path, registry_path)
    }

    fn inner(&self) -> Arc<ServerTlsConfigInner> {
        self.inner.clone()
    }
}

/// Error type returned by the HTTP server implementation.
#[derive(Debug)]
pub enum HttpError {
    Io(io::Error),
    MalformedRequestLine,
    UnsupportedMethod,
    MalformedHeader,
    MissingHost,
    UnsupportedVersion,
    BodyTooLarge,
    Timeout,
    Handler(String),
    Serialization(SerializationError),
    WebSocketUpgrade(&'static str),
    WebSocketTlsUnsupported,
}

impl From<io::Error> for HttpError {
    fn from(value: io::Error) -> Self {
        HttpError::Io(value)
    }
}

impl From<SerializationError> for HttpError {
    fn from(value: SerializationError) -> Self {
        HttpError::Serialization(value)
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpError::Io(err) => write!(f, "io error: {err}"),
            HttpError::MalformedRequestLine => write!(f, "malformed request line"),
            HttpError::UnsupportedMethod => write!(f, "unsupported http method"),
            HttpError::MalformedHeader => write!(f, "malformed header"),
            HttpError::MissingHost => write!(f, "missing host header"),
            HttpError::UnsupportedVersion => write!(f, "unsupported http version"),
            HttpError::BodyTooLarge => write!(f, "payload too large"),
            HttpError::Timeout => write!(f, "timeout"),
            HttpError::Handler(msg) => write!(f, "handler error: {msg}"),
            HttpError::Serialization(err) => write!(f, "serialization error: {err}"),
            HttpError::WebSocketUpgrade(reason) => {
                write!(f, "websocket upgrade failed: {reason}")
            }
            HttpError::WebSocketTlsUnsupported => {
                write!(f, "websocket upgrades are not supported over tls listeners")
            }
        }
    }
}

impl std::error::Error for HttpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HttpError::Io(err) => Some(err),
            HttpError::Serialization(err) => Some(err),
            _ => None,
        }
    }
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

type Handler<State> =
    Arc<dyn Fn(Request<State>) -> BoxFuture<'static, Result<Response, HttpError>> + Send + Sync>;

type UpgradeHandler<State> = Arc<
    dyn Fn(
            Request<State>,
            WebSocketRequest,
        ) -> BoxFuture<'static, Result<WebSocketResponse, HttpError>>
        + Send
        + Sync,
>;

type UpgradeCallback =
    Box<dyn FnOnce(ServerStream) -> BoxFuture<'static, Result<(), HttpError>> + Send + 'static>;

struct Route<State> {
    method: Method,
    pattern: RoutePattern,
    handler: Handler<State>,
}

struct UpgradeRoute<State> {
    method: Method,
    pattern: RoutePattern,
    handler: UpgradeHandler<State>,
}

struct RoutePattern {
    segments: Vec<Segment>,
}

enum Segment {
    Literal(String),
    Param(String),
    Wildcard(Option<String>),
}

struct TlsStream {
    stream: TcpStream,
    session: tls::SessionKeys,
    read_buffer: Vec<u8>,
    read_offset: usize,
    read_seq: u64,
    write_seq: u64,
    eof: bool,
}

impl TlsStream {
    async fn accept(mut stream: TcpStream, config: Arc<ServerTlsConfigInner>) -> io::Result<Self> {
        let outcome =
            tls::perform_handshake(&mut stream, &config.identity, &config.client_auth).await?;
        if let Some(client) = outcome.client_key {
            let encoded = base64_fp::encode_standard(&client.to_bytes());
            info!("tls client authenticated", %encoded);
        }
        Ok(Self {
            stream,
            session: outcome.session,
            read_buffer: Vec::new(),
            read_offset: 0,
            read_seq: 0,
            write_seq: 0,
            eof: false,
        })
    }

    async fn read_plain(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.read_offset < self.read_buffer.len() {
            let available = &self.read_buffer[self.read_offset..];
            let to_copy = available.len().min(buf.len());
            buf[..to_copy].copy_from_slice(&available[..to_copy]);
            self.read_offset += to_copy;
            if self.read_offset >= self.read_buffer.len() {
                self.read_buffer.clear();
                self.read_offset = 0;
            }
            return Ok(to_copy);
        }
        if self.eof {
            return Ok(0);
        }
        match self.read_record().await? {
            Some(plain) => {
                let to_copy = plain.len().min(buf.len());
                buf[..to_copy].copy_from_slice(&plain[..to_copy]);
                if to_copy < plain.len() {
                    self.read_buffer = plain;
                    self.read_offset = to_copy;
                }
                Ok(to_copy)
            }
            None => {
                self.eof = true;
                Ok(0)
            }
        }
    }

    async fn read_record(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut header = [0u8; 12];
        match self.stream.read_exact(&mut header).await {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::UnexpectedEof => return Ok(None),
            Err(err) => return Err(err),
        }
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let seq = u64::from_be_bytes(header[4..12].try_into().unwrap());
        let padded = ((length / AES_BLOCK) + 1) * AES_BLOCK;
        let mut iv = [0u8; AES_BLOCK];
        self.stream.read_exact(&mut iv).await?;
        let mut ciphertext = vec![0u8; padded];
        self.stream.read_exact(&mut ciphertext).await?;
        let mut mac = [0u8; MAC_LEN];
        self.stream.read_exact(&mut mac).await?;
        let mut frame = Vec::with_capacity(12 + AES_BLOCK + padded + MAC_LEN);
        frame.extend_from_slice(&header);
        frame.extend_from_slice(&iv);
        frame.extend_from_slice(&ciphertext);
        frame.extend_from_slice(&mac);
        let plain = tls::decrypt_record(
            &self.session.client_write,
            &self.session.client_mac,
            self.read_seq,
            &frame,
        )?;
        if seq != self.read_seq {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tls sequence mismatch",
            ));
        }
        self.read_seq = self.read_seq.wrapping_add(1);
        Ok(Some(plain))
    }

    async fn write_plain(&mut self, mut buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut written = 0usize;
        while !buf.is_empty() {
            let chunk = buf.len().min(16 * 1024);
            let frame = tls::encrypt_record(
                &self.session.server_write,
                &self.session.server_mac,
                self.write_seq,
                &buf[..chunk],
            )?;
            self.stream.write_all(&frame).await?;
            self.write_seq = self.write_seq.wrapping_add(1);
            buf = &buf[chunk..];
            written += chunk;
        }
        Ok(written)
    }

    async fn shutdown_plain(&mut self) -> io::Result<()> {
        let frame = tls::encrypt_record(
            &self.session.server_write,
            &self.session.server_mac,
            self.write_seq,
            &[],
        )?;
        self.stream.write_all(&frame).await?;
        self.stream.flush().await?;
        self.stream.shutdown().await
    }
}

impl ConnectionIo for TlsStream {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> IoFuture<'a, usize> {
        Box::pin(async move { self.read_plain(buf).await })
    }

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> IoFuture<'a, usize> {
        Box::pin(async move { self.write_plain(buf).await })
    }

    fn flush<'a>(&'a mut self) -> IoFuture<'a, ()> {
        Box::pin(async move { self.stream.flush().await })
    }

    fn shutdown<'a>(&'a mut self) -> IoFuture<'a, ()> {
        Box::pin(async move { self.shutdown_plain().await })
    }
}

impl UpgradeIo for TlsStream {
    type WebSocket = Self;

    fn supports_websocket(&self) -> bool {
        false
    }

    fn into_websocket(self) -> Result<Self::WebSocket, HttpError> {
        Err(HttpError::WebSocketTlsUnsupported)
    }
}

impl ws::WebSocketIo for TlsStream {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> ws::IoFuture<'a, usize> {
        Box::pin(async move { self.read_plain(buf).await })
    }

    fn write<'a>(&'a mut self, buf: &'a [u8]) -> ws::IoFuture<'a, usize> {
        Box::pin(async move { self.write_plain(buf).await })
    }

    fn flush<'a>(&'a mut self) -> ws::IoFuture<'a, ()> {
        Box::pin(async move { self.stream.flush().await })
    }

    fn shutdown<'a>(&'a mut self) -> ws::IoFuture<'a, ()> {
        Box::pin(async move { self.shutdown_plain().await })
    }
}

impl<State> Clone for Route<State> {
    fn clone(&self) -> Self {
        Self {
            method: self.method,
            pattern: self.pattern.clone(),
            handler: self.handler.clone(),
        }
    }
}

impl<State> Clone for UpgradeRoute<State> {
    fn clone(&self) -> Self {
        Self {
            method: self.method,
            pattern: self.pattern.clone(),
            handler: self.handler.clone(),
        }
    }
}

impl Clone for RoutePattern {
    fn clone(&self) -> Self {
        Self {
            segments: self.segments.clone(),
        }
    }
}

impl Clone for Segment {
    fn clone(&self) -> Self {
        match self {
            Segment::Literal(val) => Segment::Literal(val.clone()),
            Segment::Param(val) => Segment::Param(val.clone()),
            Segment::Wildcard(name) => Segment::Wildcard(name.clone()),
        }
    }
}

impl RoutePattern {
    fn parse(pattern: &str) -> Self {
        let mut segments = Vec::new();
        let mut saw_wildcard = false;
        for segment in pattern.trim_start_matches('/').split('/') {
            if segment.is_empty() {
                continue;
            }
            if saw_wildcard {
                break;
            }
            if let Some(name) = segment.strip_prefix(':') {
                segments.push(Segment::Param(name.to_string()));
                continue;
            }
            if let Some(name) = segment.strip_prefix('*') {
                let wildcard_name = if name.is_empty() {
                    None
                } else {
                    Some(name.to_string())
                };
                segments.push(Segment::Wildcard(wildcard_name));
                saw_wildcard = true;
                continue;
            }
            segments.push(Segment::Literal(segment.to_string()));
        }
        Self { segments }
    }

    fn matches(&self, path: &str, params: &mut HashMap<String, String>) -> bool {
        let mut param_buf = HashMap::new();
        let segments: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let mut index = 0usize;
        for segment in &self.segments {
            match segment {
                Segment::Literal(lit) => {
                    let Some(actual) = segments.get(index) else {
                        return false;
                    };
                    if lit != actual {
                        return false;
                    }
                    index += 1;
                }
                Segment::Param(name) => {
                    let Some(actual) = segments.get(index) else {
                        return false;
                    };
                    param_buf.insert(name.clone(), (*actual).to_string());
                    index += 1;
                }
                Segment::Wildcard(name) => {
                    let remainder = segments[index..].join("/");
                    if let Some(key) = name {
                        param_buf.insert(key.clone(), remainder);
                    }
                    params.extend(param_buf);
                    return true;
                }
            }
        }
        if index != segments.len() {
            return false;
        }
        params.extend(param_buf);
        true
    }
}

/// HTTP request abstraction exposed to handlers.
pub struct Request<State> {
    method: Method,
    path: String,
    query: HashMap<String, String>,
    version: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    params: HashMap<String, String>,
    state: Arc<State>,
    remote_addr: SocketAddr,
    keep_alive: bool,
}

impl<State> Request<State> {
    fn new(
        method: Method,
        target: String,
        version: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
        params: HashMap<String, String>,
        state: Arc<State>,
        remote_addr: SocketAddr,
        keep_alive: bool,
    ) -> Self {
        let (clean_path, query) = match target.split_once('?') {
            Some((path, qs)) => {
                let mut map = HashMap::new();
                for (key, value) in form_urlencoded::parse(qs.as_bytes()) {
                    map.insert(key, value);
                }
                (path.to_string(), map)
            }
            None => (target, HashMap::new()),
        };
        Self {
            method,
            path: clean_path,
            query,
            version,
            headers,
            body,
            params,
            state,
            remote_addr,
            keep_alive,
        }
    }

    pub fn method(&self) -> Method {
        self.method
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }

    pub fn query(&self) -> &HashMap<String, String> {
        &self.query
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(|s| s.as_str())
    }

    pub fn headers(&self) -> &HashMap<String, String> {
        &self.headers
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    pub fn state(&self) -> &Arc<State> {
        &self.state
    }

    pub fn keep_alive(&self) -> bool {
        self.keep_alive
    }

    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T, HttpError> {
        json::from_slice(&self.body).map_err(HttpError::from)
    }

    pub fn take_body(mut self) -> Vec<u8> {
        std::mem::take(&mut self.body)
    }

    fn set_params(&mut self, params: HashMap<String, String>) {
        self.params = params;
    }
}

/// Builder used to construct [`Request`] values for unit tests and direct
/// handler invocation without a socket.
pub struct RequestBuilder<State> {
    method: Method,
    path: String,
    query: Vec<(String, String)>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    state: Arc<State>,
    remote_addr: SocketAddr,
    keep_alive: bool,
    version: String,
}

impl<State> RequestBuilder<State> {
    fn new(state: Arc<State>) -> Self {
        Self {
            method: Method::Get,
            path: "/".to_string(),
            query: Vec::new(),
            headers: HashMap::new(),
            body: Vec::new(),
            state,
            remote_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            keep_alive: true,
            version: "HTTP/1.1".to_string(),
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.method = method;
        self
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        self.path = if path.is_empty() {
            "/".to_string()
        } else {
            path
        };
        self
    }

    pub fn query_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.query.push((key.into(), value.into()));
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(name.into().to_ascii_lowercase(), value.into());
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn json<T: Serialize>(mut self, value: &T) -> Result<Self, HttpError> {
        self.body = json::to_vec(value)?;
        self.headers
            .insert("content-type".into(), "application/json".into());
        Ok(self)
    }

    pub fn remote_addr(mut self, remote: SocketAddr) -> Self {
        self.remote_addr = remote;
        self
    }

    pub fn keep_alive(mut self, keep_alive: bool) -> Self {
        self.keep_alive = keep_alive;
        self
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.headers.insert("host".into(), host.into());
        self
    }

    pub fn build(self) -> Request<State> {
        let mut headers = self.headers;
        if !headers.contains_key("host") {
            headers.insert("host".into(), "localhost".into());
        }
        let mut target = if self.path.is_empty() {
            "/".to_string()
        } else {
            self.path
        };
        if !self.query.is_empty() {
            let mut serializer = form_urlencoded::Serializer::new(String::new());
            for (key, value) in &self.query {
                serializer.append_pair(key, value);
            }
            target.push('?');
            target.push_str(&serializer.finish());
        }
        Request::new(
            self.method,
            target,
            self.version,
            headers,
            self.body,
            HashMap::new(),
            self.state,
            self.remote_addr,
            self.keep_alive,
        )
    }
}

/// Metadata extracted from a WebSocket upgrade request.
pub struct WebSocketRequest {
    key: String,
    protocols: Vec<String>,
}

impl WebSocketRequest {
    fn new(key: String, protocols: Vec<String>) -> Self {
        Self { key, protocols }
    }

    /// Returns the Sec-WebSocket-Accept value derived from the client key.
    fn accept_value(&self) -> Result<String, HttpError> {
        ws::handshake_accept(&self.key).map_err(|err| HttpError::Handler(err.to_string()))
    }

    /// Returns the list of requested subprotocols in the order supplied by
    /// the client.
    pub fn protocols(&self) -> &[String] {
        &self.protocols
    }

    /// Returns `true` when the client advertised the provided subprotocol.
    pub fn has_protocol(&self, protocol: &str) -> bool {
        self.protocols
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(protocol))
    }
}

/// Decision returned by WebSocket upgrade handlers.
pub enum WebSocketResponse {
    Upgrade {
        headers: Vec<(String, String)>,
        on_upgrade: UpgradeCallback,
    },
    Reject(Response),
}

impl WebSocketResponse {
    pub fn accept<F, Fut>(on_upgrade: F) -> Self
    where
        F: FnOnce(ServerStream) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), HttpError>> + Send + 'static,
    {
        Self::Upgrade {
            headers: Vec::new(),
            on_upgrade: Box::new(move |stream| Box::pin(on_upgrade(stream))),
        }
    }

    pub fn reject(response: Response) -> Self {
        Self::Reject(response)
    }

    pub fn with_protocol(mut self, protocol: impl Into<String>) -> Self {
        self = self.with_header("sec-websocket-protocol", protocol);
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        if let Self::Upgrade { headers, .. } = &mut self {
            headers.push((name.into().to_ascii_lowercase(), value.into()));
        }
        self
    }
}

/// HTTP response abstraction used by handlers.
pub struct Response {
    status: StatusCode,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    keep_alive: bool,
}

impl Response {
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: Vec::new(),
            keep_alive: true,
        }
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(name.into().to_ascii_lowercase(), value.into());
        self
    }

    pub fn with_connection(mut self, keep_alive: bool) -> Self {
        self.keep_alive = keep_alive;
        self
    }

    pub fn close(self) -> Self {
        self.with_connection(false)
    }

    pub fn json<T: Serialize>(mut self, value: &T) -> Result<Self, HttpError> {
        self.body = json::to_vec(value)?;
        self.headers
            .insert("content-type".into(), "application/json".into());
        Ok(self)
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn is_keep_alive(&self) -> bool {
        self.keep_alive
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

impl Clone for Response {
    fn clone(&self) -> Self {
        Self {
            status: self.status,
            headers: self.headers.clone(),
            body: self.body.clone(),
            keep_alive: self.keep_alive,
        }
    }
}

/// Router structure used to dispatch requests to handlers.
pub struct Router<State> {
    routes: Arc<Vec<Route<State>>>,
    upgrades: Arc<Vec<UpgradeRoute<State>>>,
    state: Arc<State>,
}

impl<State> Router<State>
where
    State: Send + Sync + 'static,
{
    pub fn new(state: State) -> Self {
        Self {
            routes: Arc::new(Vec::new()),
            upgrades: Arc::new(Vec::new()),
            state: Arc::new(state),
        }
    }

    fn with_routes(
        state: Arc<State>,
        routes: Vec<Route<State>>,
        upgrades: Vec<UpgradeRoute<State>>,
    ) -> Self {
        Self {
            routes: Arc::new(routes),
            upgrades: Arc::new(upgrades),
            state,
        }
    }

    pub fn request_builder(&self) -> RequestBuilder<State> {
        RequestBuilder::new(self.state.clone())
    }

    pub async fn handle(&self, request: Request<State>) -> Result<Response, HttpError> {
        self.dispatch(request).await
    }

    pub fn get<F, Fut>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(Request<State>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, HttpError>> + Send + 'static,
    {
        self.add_route(Method::Get, pattern, handler)
    }

    pub fn post<F, Fut>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(Request<State>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, HttpError>> + Send + 'static,
    {
        self.add_route(Method::Post, pattern, handler)
    }

    pub fn route<F, Fut>(self, method: Method, pattern: &str, handler: F) -> Self
    where
        F: Fn(Request<State>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, HttpError>> + Send + 'static,
    {
        self.add_route(method, pattern, handler)
    }

    fn add_route<F, Fut>(self, method: Method, pattern: &str, handler: F) -> Self
    where
        F: Fn(Request<State>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, HttpError>> + Send + 'static,
    {
        let handler: Handler<State> = Arc::new(move |req| {
            let fut = handler(req);
            Box::pin(fut)
        });
        let mut routes = (*self.routes).clone();
        routes.push(Route {
            method,
            pattern: RoutePattern::parse(pattern),
            handler,
        });
        Router::with_routes(self.state.clone(), routes, (*self.upgrades).clone())
    }

    pub fn upgrade<F, Fut>(self, pattern: &str, handler: F) -> Self
    where
        F: Fn(Request<State>, WebSocketRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<WebSocketResponse, HttpError>> + Send + 'static,
    {
        let handler: UpgradeHandler<State> = Arc::new(move |req, upgrade| {
            let fut = handler(req, upgrade);
            Box::pin(fut)
        });
        let mut upgrades = (*self.upgrades).clone();
        upgrades.push(UpgradeRoute {
            method: Method::Get,
            pattern: RoutePattern::parse(pattern),
            handler,
        });
        Router::with_routes(self.state.clone(), (*self.routes).clone(), upgrades)
    }

    async fn dispatch(&self, request: Request<State>) -> Result<Response, HttpError> {
        for route in self.routes.iter() {
            if route.method != request.method {
                continue;
            }
            let mut params = HashMap::new();
            if route.pattern.matches(request.path(), &mut params) {
                let mut request = request;
                request.set_params(params);
                return (route.handler)(request).await;
            }
        }
        Ok(Response::new(StatusCode::NOT_FOUND).with_body(Vec::new()))
    }

    fn match_upgrade(
        &self,
        request: &Request<State>,
    ) -> Option<(UpgradeHandler<State>, HashMap<String, String>)> {
        for route in self.upgrades.iter() {
            if route.method != request.method() {
                continue;
            }
            let mut params = HashMap::new();
            if route.pattern.matches(request.path(), &mut params) {
                return Some((route.handler.clone(), params));
            }
        }
        None
    }
}

impl<State> Clone for Router<State> {
    fn clone(&self) -> Self {
        Self {
            routes: self.routes.clone(),
            upgrades: self.upgrades.clone(),
            state: self.state.clone(),
        }
    }
}

/// Starts serving HTTP requests using the provided router and configuration.
pub async fn serve<State>(
    listener: TcpListener,
    router: Router<State>,
    config: ServerConfig,
) -> io::Result<()>
where
    State: Send + Sync + 'static,
{
    loop {
        let (stream, remote) = listener.accept().await?;
        let router = router.clone();
        let config = config.clone();
        spawn(async move {
            if let Err(err) = handle_connection(stream, remote, router, config).await {
                debug!(?err, "http connection error");
            }
        });
    }
}

/// Serves HTTPS traffic by completing TLS handshakes before dispatching to the
/// HTTP request loop.
pub async fn serve_tls<State>(
    listener: TcpListener,
    router: Router<State>,
    config: ServerConfig,
    tls: ServerTlsConfig,
) -> io::Result<()>
where
    State: Send + Sync + 'static,
{
    loop {
        let (stream, remote) = listener.accept().await?;
        let router = router.clone();
        let config = config.clone();
        let tls = tls.clone();
        spawn(async move {
            match TlsStream::accept(stream, tls.inner()).await {
                Ok(tls_stream) => {
                    if let Err(err) = handle_connection(tls_stream, remote, router, config).await {
                        debug!(?err, "http tls connection error");
                    }
                }
                Err(err) => {
                    debug!(?err, "tls handshake failed");
                }
            }
        });
    }
}

async fn handle_connection<State, S>(
    stream: S,
    remote: SocketAddr,
    router: Router<State>,
    config: ServerConfig,
) -> Result<(), HttpError>
where
    State: Send + Sync + 'static,
    S: ConnectionIo + UpgradeIo,
{
    let mut stream = BufferedStream::new(stream);
    let mut idle_deadline = config.request_timeout;
    loop {
        let request = match timeout(
            idle_deadline,
            read_request(
                &mut stream,
                remote,
                router.state.clone(),
                config.max_body_bytes,
            ),
        )
        .await
        {
            Ok(res) => res?,
            Err(_) => return Err(HttpError::Timeout),
        };
        let Some(request) = request else {
            return Ok(());
        };
        idle_deadline = config.request_timeout;
        let mut request = request;
        let keep_alive = request.keep_alive();
        let keep_alive_allowed = keep_alive && !config.keep_alive.is_zero();
        if let Some((upgrade_handler, params)) = router.match_upgrade(&request) {
            request.set_params(params);
            let handshake = match parse_websocket_request(&request) {
                Ok(info) => info,
                Err(err) => {
                    let response = Response::new(StatusCode::BAD_REQUEST)
                        .with_body(err.to_string().into_bytes())
                        .close();
                    timeout(config.request_timeout, async {
                        stream.write_all(&serialize_response(&response)?).await
                    })
                    .await
                    .map_err(|_| HttpError::Timeout)??;
                    stream.shutdown().await?;
                    return Ok(());
                }
            };
            if !stream.inner_ref().supports_websocket() {
                let response = Response::new(StatusCode::BAD_REQUEST)
                    .with_body(b"websocket upgrades require plaintext listeners".to_vec())
                    .close();
                timeout(config.request_timeout, async {
                    stream.write_all(&serialize_response(&response)?).await
                })
                .await
                .map_err(|_| HttpError::Timeout)??;
                stream.shutdown().await?;
                return Ok(());
            }
            let accept_header = handshake.accept_value()?;
            let decision = (upgrade_handler)(request, handshake).await?;
            match decision {
                WebSocketResponse::Reject(mut response) => {
                    if !keep_alive_allowed {
                        response = response.close();
                    } else if response.is_keep_alive() {
                        if config.keep_alive.as_secs() > 0 {
                            response = response.with_header(
                                "keep-alive",
                                format!("timeout={}", config.keep_alive.as_secs()),
                            );
                        }
                        response = response.with_connection(true);
                    }
                    timeout(config.request_timeout, async {
                        stream.write_all(&serialize_response(&response)?).await
                    })
                    .await
                    .map_err(|_| HttpError::Timeout)??;
                    if !response.is_keep_alive() {
                        stream.shutdown().await?;
                        break;
                    }
                    if keep_alive_allowed {
                        idle_deadline = config.keep_alive;
                    }
                    continue;
                }
                WebSocketResponse::Upgrade {
                    headers,
                    on_upgrade,
                } => {
                    let mut response = Response::new(StatusCode::SWITCHING_PROTOCOLS)
                        .with_header("upgrade", "websocket")
                        .with_header("connection", "Upgrade")
                        .with_header("sec-websocket-accept", accept_header);
                    for (name, value) in headers {
                        response = response.with_header(name, value);
                    }
                    timeout(config.request_timeout, async {
                        stream.write_all(&serialize_response(&response)?).await
                    })
                    .await
                    .map_err(|_| HttpError::Timeout)??;
                    let raw_stream = stream.into_inner().into_websocket()?;
                    spawn(async move {
                        if let Err(err) = on_upgrade(ServerStream::from_io(raw_stream)).await {
                            debug!(?err, "websocket handler error");
                        }
                    });
                    return Ok(());
                }
            }
        }
        let mut response = router.handle(request).await?;
        if !keep_alive_allowed {
            response = response.close();
        } else if response.is_keep_alive() {
            if config.keep_alive.as_secs() > 0 {
                response = response.with_header(
                    "keep-alive",
                    format!("timeout={}", config.keep_alive.as_secs()),
                );
            }
            response = response.with_connection(true);
        }
        timeout(config.request_timeout, async {
            stream.write_all(&serialize_response(&response)?).await
        })
        .await
        .map_err(|_| HttpError::Timeout)??;
        if !response.is_keep_alive() {
            stream.shutdown().await?;
            break;
        }
        if keep_alive_allowed {
            idle_deadline = config.keep_alive;
        }
    }
    Ok(())
}

fn serialize_response(response: &Response) -> io::Result<Vec<u8>> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status.0,
        response.status.reason()
    )
    .into_bytes();
    let mut headers = response.headers.clone();
    headers.insert("content-length".into(), response.body.len().to_string());
    if !headers.contains_key("connection") {
        if response.keep_alive {
            headers.insert("connection".into(), "keep-alive".into());
        } else {
            headers.insert("connection".into(), "close".into());
        }
    }
    for (name, value) in headers.iter() {
        head.extend_from_slice(name.as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(value.as_bytes());
        head.extend_from_slice(b"\r\n");
    }
    head.extend_from_slice(b"\r\n");
    head.extend_from_slice(&response.body);
    Ok(head)
}

fn parse_websocket_request<State>(request: &Request<State>) -> Result<WebSocketRequest, HttpError> {
    if request.method() != Method::Get {
        return Err(HttpError::WebSocketUpgrade(
            "websocket upgrades require GET",
        ));
    }
    let Some(upgrade) = request.header("upgrade") else {
        return Err(HttpError::WebSocketUpgrade("missing upgrade header"));
    };
    if !upgrade.eq_ignore_ascii_case("websocket") {
        return Err(HttpError::WebSocketUpgrade(
            "upgrade header must be websocket",
        ));
    }
    let connection = request
        .header("connection")
        .unwrap_or("")
        .split(',')
        .any(|token| token.trim().eq_ignore_ascii_case("upgrade"));
    if !connection {
        return Err(HttpError::WebSocketUpgrade(
            "connection header must include upgrade",
        ));
    }
    let Some(key) = request.header("sec-websocket-key") else {
        return Err(HttpError::WebSocketUpgrade("missing Sec-WebSocket-Key"));
    };
    let version = request.header("sec-websocket-version").unwrap_or("13");
    if version != "13" {
        return Err(HttpError::WebSocketUpgrade("unsupported websocket version"));
    }
    let protocols = request
        .header("sec-websocket-protocol")
        .map(|value| {
            value
                .split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(WebSocketRequest::new(key.to_string(), protocols))
}

async fn read_request<State, S>(
    stream: &mut BufferedStream<S>,
    remote: SocketAddr,
    state: Arc<State>,
    max_body: usize,
) -> Result<Option<Request<State>>, HttpError>
where
    State: Send + Sync + 'static,
    S: ConnectionIo,
{
    let mut line = String::new();
    let read = stream.read_line(&mut line).await?;
    if read == 0 {
        return Ok(None);
    }
    let parts: Vec<&str> = line
        .trim_end_matches(['\r', '\n'])
        .split_whitespace()
        .collect();
    if parts.len() != 3 {
        return Err(HttpError::MalformedRequestLine);
    }
    let method = Method::from_bytes(parts[0].as_bytes())?;
    let target = parts[1].to_string();
    let version = parts[2].to_string();
    if version != "HTTP/1.1" {
        return Err(HttpError::UnsupportedVersion);
    }
    let mut headers = HashMap::new();
    loop {
        let mut header_line = String::new();
        stream.read_line(&mut header_line).await?;
        if header_line == "\r\n" || header_line == "\n" {
            break;
        }
        if let Some((name, value)) = header_line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        } else {
            return Err(HttpError::MalformedHeader);
        }
    }
    let host_present = headers.contains_key("host");
    if !host_present {
        return Err(HttpError::MissingHost);
    }
    let keep_alive = headers
        .get("connection")
        .map(|v| v.eq_ignore_ascii_case("keep-alive"))
        .unwrap_or(true);
    let content_length = headers
        .get("content-length")
        .map(|v| v.parse::<usize>())
        .transpose()
        .map_err(|_| HttpError::MalformedHeader)?
        .unwrap_or(0);
    if content_length > max_body {
        return Err(HttpError::BodyTooLarge);
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream.read_exact(&mut body).await?;
    }
    let request = Request::new(
        method,
        target,
        version,
        headers,
        body,
        HashMap::new(),
        state,
        remote,
        keep_alive,
    );
    Ok(Some(request))
}
