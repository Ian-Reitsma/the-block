use crate::{WalletError, WalletSigner};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use crypto_suite::signatures::ed25519::{Signature, VerifyingKey, SIGNATURE_LENGTH};
use hex;
use httpd::{BlockingClient, Method};
use ledger::crypto::remote_tag;
use metrics::{histogram, increment_counter};
use native_tls::{Certificate as NativeCertificate, HandshakeError, Identity, TlsConnector};
use once_cell::sync::Lazy;
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{ToSocketAddrs, UdpSocket};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use url::Url;
use uuid::Uuid;

/// Cache of signer public keys with an expiry.
static PUBKEY_CACHE: Lazy<Mutex<HashMap<String, (VerifyingKey, Instant)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
const PUBKEY_TTL: Duration = Duration::from_secs(600);
const DISCOVERY_PORT: u16 = 7878;

#[derive(Deserialize)]
struct PubKeyResp {
    pubkey: String,
}

#[derive(Serialize)]
struct SignReq<'a> {
    trace: &'a str,
    msg: String,
}

#[derive(Deserialize)]
struct SignResp {
    sig: String,
}

/// Remote signer supporting HTTP and WebSocket transports with optional
/// multisignature aggregation.
pub struct RemoteSigner {
    endpoints: Vec<String>,
    client: BlockingClient,
    tls: Option<TlsConnector>,
    pubkeys: Vec<VerifyingKey>,
    timeout: Duration,
    retries: u8,
    threshold: usize,
}

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

struct Frame {
    opcode: u8,
    fin: bool,
    payload: Vec<u8>,
}

enum StreamKind {
    Plain(std::net::TcpStream),
    Tls(native_tls::TlsStream<std::net::TcpStream>),
}

impl StreamKind {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            StreamKind::Plain(stream) => stream.write_all(buf),
            StreamKind::Tls(stream) => stream.write_all(buf),
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        match self {
            StreamKind::Plain(stream) => stream.read_exact(buf),
            StreamKind::Tls(stream) => stream.read_exact(buf),
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            StreamKind::Plain(stream) => stream.read(buf),
            StreamKind::Tls(stream) => stream.read(buf),
        }
    }
}

struct BlockingWebSocket {
    stream: StreamKind,
}

impl BlockingWebSocket {
    fn plain(stream: std::net::TcpStream) -> Self {
        Self {
            stream: StreamKind::Plain(stream),
        }
    }

    fn tls(stream: native_tls::TlsStream<std::net::TcpStream>) -> Self {
        Self {
            stream: StreamKind::Tls(stream),
        }
    }

    fn handshake(&mut self, host: &str, path: &str) -> io::Result<()> {
        let mut key_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut key_bytes);
        let key = BASE64.encode(key_bytes);
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        self.stream.write_all(request.as_bytes())?;
        self.read_handshake_response(&key)
    }

    fn read_handshake_response(&mut self, key: &str) -> io::Result<()> {
        let expected_accept = handshake_accept(key);
        let mut buf = Vec::with_capacity(512);
        let mut tmp = [0u8; 128];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            let read = self.stream.read(&mut tmp)?;
            if read == 0 {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "websocket handshake ended prematurely",
                ));
            }
            buf.extend_from_slice(&tmp[..read]);
            if buf.len() > 8192 {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "websocket handshake headers too large",
                ));
            }
        }
        let text = String::from_utf8(buf)
            .map_err(|_| io::Error::new(ErrorKind::InvalidData, "invalid utf8 in handshake"))?;
        if !text.starts_with("HTTP/1.1 101") {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "server declined websocket upgrade",
            ));
        }
        for line in text.lines() {
            if let Some((name, value)) = line.split_once(':') {
                if name.trim().eq_ignore_ascii_case("Sec-WebSocket-Accept")
                    && value.trim() == expected_accept
                {
                    return Ok(());
                }
            }
        }
        Err(io::Error::new(
            ErrorKind::InvalidData,
            "websocket accept key mismatch",
        ))
    }

    fn send_text(&mut self, text: &str) -> io::Result<()> {
        self.write_frame(0x1, text.as_bytes())
    }

    fn read_text(&mut self) -> io::Result<String> {
        loop {
            let frame = self.read_frame()?;
            if !frame.fin {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "fragmented frames are not supported",
                ));
            }
            match frame.opcode {
                0x1 => {
                    return String::from_utf8(frame.payload).map_err(|_| {
                        io::Error::new(ErrorKind::InvalidData, "invalid utf8 payload")
                    });
                }
                0x8 => {
                    return Err(io::Error::new(
                        ErrorKind::ConnectionAborted,
                        "websocket connection closed",
                    ));
                }
                0x9 => {
                    self.write_frame(0xA, &frame.payload)?;
                }
                0xA => {}
                _ => {}
            }
        }
    }

    fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> io::Result<()> {
        let mut header = Vec::with_capacity(10);
        header.push(0x80 | (opcode & 0x0F));
        let len = payload.len() as u64;
        if len < 126 {
            header.push(0x80 | len as u8);
        } else if len <= u16::MAX as u64 {
            header.push(0xFE);
            header.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            header.push(0xFF);
            header.extend_from_slice(&(len as u64).to_be_bytes());
        }
        let mut mask = [0u8; 4];
        rand::thread_rng().fill_bytes(&mut mask);
        header.extend_from_slice(&mask);
        self.stream.write_all(&header)?;
        if !payload.is_empty() {
            let mut masked = payload.to_vec();
            for (i, byte) in masked.iter_mut().enumerate() {
                *byte ^= mask[i % 4];
            }
            self.stream.write_all(&masked)?;
        }
        Ok(())
    }

    fn read_frame(&mut self) -> io::Result<Frame> {
        let mut header = [0u8; 2];
        self.stream.read_exact(&mut header)?;
        let fin = header[0] & 0x80 != 0;
        let opcode = header[0] & 0x0F;
        let masked = header[1] & 0x80 != 0;
        if masked {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "server frames must not be masked",
            ));
        }
        let mut len = (header[1] & 0x7F) as u64;
        if len == 126 {
            let mut extended = [0u8; 2];
            self.stream.read_exact(&mut extended)?;
            len = u16::from_be_bytes(extended) as u64;
        } else if len == 127 {
            let mut extended = [0u8; 8];
            self.stream.read_exact(&mut extended)?;
            len = u64::from_be_bytes(extended);
        }
        if len > (1 << 31) {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "websocket frame exceeds 2 GiB limit",
            ));
        }
        let mut payload = vec![0u8; len as usize];
        if len > 0 {
            self.stream.read_exact(&mut payload)?;
        }
        Ok(Frame {
            opcode,
            fin,
            payload,
        })
    }
}

fn handshake_accept(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WS_GUID.as_bytes());
    BASE64.encode(hasher.finalize())
}

impl RemoteSigner {
    /// Discover remote signers on the local network using a UDP broadcast.
    pub fn discover(timeout: Duration) -> Vec<String> {
        let socket = match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let _ = socket.set_broadcast(true);
        let _ = socket.send_to(b"theblock:signer?", ("255.255.255.255", DISCOVERY_PORT));
        let _ = socket.set_read_timeout(Some(timeout));
        let mut buf = [0u8; 64];
        let mut out = Vec::new();
        loop {
            match socket.recv_from(&mut buf) {
                Ok((n, src)) => {
                    if &buf[..n] == b"theblock:signer!" {
                        out.push(format!("http://{}:{DISCOVERY_PORT}", src.ip()));
                    }
                }
                Err(e) => match e.kind() {
                    ErrorKind::WouldBlock | ErrorKind::TimedOut => break,
                    _ => break,
                },
            }
        }
        out
    }

    fn fetch_pubkey(
        client: &BlockingClient,
        endpoint: &str,
        tls: Option<&TlsConnector>,
    ) -> Result<VerifyingKey, WalletError> {
        {
            let cache = PUBKEY_CACHE.lock().unwrap();
            if let Some((pk, ts)) = cache.get(endpoint) {
                if ts.elapsed() < PUBKEY_TTL {
                    return Ok(pk.clone());
                }
            }
        }
        let mut url = Url::parse(endpoint)
            .map_err(|err| WalletError::Failure(format!("invalid signer url: {err}")))?;
        match url.scheme() {
            "ws" => {
                url.set_scheme("http")
                    .map_err(|_| WalletError::Failure("invalid ws url".into()))?;
            }
            "wss" => {
                url.set_scheme("https")
                    .map_err(|_| WalletError::Failure("invalid wss url".into()))?;
            }
            "http" | "https" => {}
            scheme => {
                return Err(WalletError::Failure(format!(
                    "unsupported signer scheme: {scheme}"
                )));
            }
        }
        let pubkey_url = url
            .join("pubkey")
            .map_err(|err| WalletError::Failure(err.to_string()))?;
        let pk: PubKeyResp = if pubkey_url.scheme() == "https" {
            fetch_pubkey_https(&pubkey_url, tls)?
        } else {
            let resp = client
                .request(Method::Get, pubkey_url.as_str())
                .map_err(|e| WalletError::Failure(e.to_string()))?
                .timeout(Duration::from_secs(5))
                .send()
                .map_err(|e| WalletError::Failure(e.to_string()))?;
            resp.json()
                .map_err(|e| WalletError::Failure(e.to_string()))?
        };
        let bytes = hex::decode(pk.pubkey).map_err(|e| WalletError::Failure(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(WalletError::Failure("invalid pubkey length".into()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        let pubkey =
            VerifyingKey::from_bytes(&arr).map_err(|e| WalletError::Failure(e.to_string()))?;
        PUBKEY_CACHE
            .lock()
            .unwrap()
            .insert(endpoint.to_string(), (pubkey.clone(), Instant::now()));
        increment_counter!("remote_signer_key_rotation_total");
        Ok(pubkey)
    }

    /// Connect to one or more signer endpoints with a threshold.
    pub fn connect_multi(endpoints: &[String], threshold: usize) -> Result<Self, WalletError> {
        if endpoints.is_empty() || threshold == 0 || threshold > endpoints.len() {
            return Err(WalletError::Failure("invalid signer configuration".into()));
        }
        let timeout_ms = std::env::var("REMOTE_SIGNER_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5_000);
        let timeout = Duration::from_millis(timeout_ms);
        let client = BlockingClient::default();
        let mut tls = None;
        if let (Ok(cert_path), Ok(key_path)) = (
            std::env::var("REMOTE_SIGNER_TLS_CERT"),
            std::env::var("REMOTE_SIGNER_TLS_KEY"),
        ) {
            let cert_bytes =
                fs::read(cert_path).map_err(|e| WalletError::Failure(e.to_string()))?;
            let key_bytes = fs::read(key_path).map_err(|e| WalletError::Failure(e.to_string()))?;
            let identity = Identity::from_pkcs8(&cert_bytes, &key_bytes)
                .map_err(|e| WalletError::Failure(e.to_string()))?;
            let mut tls_builder = TlsConnector::builder();
            tls_builder.identity(identity.clone());
            if let Ok(ca_path) = std::env::var("REMOTE_SIGNER_TLS_CA") {
                let ca_bytes =
                    fs::read(ca_path).map_err(|e| WalletError::Failure(e.to_string()))?;
                let ca_cert = NativeCertificate::from_pem(&ca_bytes)
                    .map_err(|e| WalletError::Failure(e.to_string()))?;
                tls_builder.add_root_certificate(ca_cert);
                tls_builder.danger_accept_invalid_certs(true);
            }
            let connector = tls_builder
                .build()
                .map_err(|e| WalletError::Failure(e.to_string()))?;
            tls = Some(connector);
        }
        let mut pubkeys = Vec::new();
        for ep in endpoints {
            pubkeys.push(Self::fetch_pubkey(&client, ep, tls.as_ref())?);
        }
        Ok(Self {
            endpoints: endpoints.to_vec(),
            client,
            tls,
            pubkeys,
            timeout,
            retries: 3,
            threshold,
        })
    }

    /// Connect to a single signer.
    pub fn connect(endpoint: &str) -> Result<Self, WalletError> {
        Self::connect_multi(&[endpoint.to_string()], 1)
    }

    /// Return the configured threshold for approvals.
    pub fn threshold(&self) -> usize {
        self.threshold
    }

    fn sign_http(&self, endpoint: &str, payload: &SignReq) -> Result<Signature, WalletError> {
        for attempt in 0..=self.retries {
            info!(%payload.trace, attempt, "remote sign request");
            let res = self
                .client
                .request(Method::Post, &format!("{endpoint}/sign"))
                .map_err(|e| WalletError::Failure(e.to_string()))?
                .timeout(self.timeout)
                .json(payload)
                .map_err(|e| WalletError::Failure(e.to_string()))?
                .send();
            match res {
                Ok(resp) => match resp.json::<SignResp>() {
                    Ok(r) => {
                        let sig_bytes =
                            hex::decode(r.sig).map_err(|e| WalletError::Failure(e.to_string()))?;
                        if sig_bytes.len() != SIGNATURE_LENGTH {
                            return Err(WalletError::Failure("invalid signature length".into()));
                        }
                        let sig_arr: [u8; SIGNATURE_LENGTH] = sig_bytes
                            .as_slice()
                            .try_into()
                            .map_err(|_| WalletError::Failure("invalid signature".into()))?;
                        return Ok(Signature::from_bytes(&sig_arr));
                    }
                    Err(e) => {
                        if attempt == self.retries {
                            return Err(WalletError::Failure(e.to_string()));
                        }
                        warn!(%payload.trace, error=%e, "retrying signer parse");
                    }
                },
                Err(e) => {
                    if attempt == self.retries {
                        if e.is_timeout() {
                            return Err(WalletError::Timeout);
                        }
                        return Err(WalletError::Failure(e.to_string()));
                    }
                    warn!(%payload.trace, error=%e, "retrying signer request");
                }
            }
        }
        Err(WalletError::Failure("unreachable".into()))
    }

    fn sign_ws(&self, endpoint: &str, payload: &SignReq) -> Result<Signature, WalletError> {
        let url = format!("{endpoint}/sign");
        let url = Url::parse(&url).map_err(|e| WalletError::Failure(e.to_string()))?;
        let host = url
            .host_str()
            .ok_or_else(|| WalletError::Failure("missing host".into()))?;
        let port = url.port_or_known_default().unwrap_or(443);
        let addr = format!("{host}:{port}");
        let tcp =
            std::net::TcpStream::connect(&addr).map_err(|e| WalletError::Failure(e.to_string()))?;
        let mut ws = if let Some(connector) = &self.tls {
            let tls_stream = connector
                .connect(host, tcp)
                .map_err(|e| WalletError::Failure(e.to_string()))?;
            BlockingWebSocket::tls(tls_stream)
        } else {
            BlockingWebSocket::plain(tcp)
        };

        let mut path = url.path().to_string();
        if path.is_empty() {
            path.push('/');
        }
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }
        let host_header = if let Some(port) = url.port() {
            format!("{host}:{port}")
        } else {
            host.to_string()
        };

        ws.handshake(&host_header, &path)
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        ws.send_text(&serde_json::to_string(payload).unwrap())
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let txt = ws
            .read_text()
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let r: SignResp =
            serde_json::from_str(&txt).map_err(|e| WalletError::Failure(e.to_string()))?;
        let sig_bytes = hex::decode(r.sig).map_err(|e| WalletError::Failure(e.to_string()))?;
        if sig_bytes.len() != SIGNATURE_LENGTH {
            return Err(WalletError::Failure("invalid signature length".into()));
        }
        let sig_arr: [u8; SIGNATURE_LENGTH] = sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| WalletError::Failure("invalid signature".into()))?;
        Ok(Signature::from_bytes(&sig_arr))
    }
}

fn fetch_pubkey_https(url: &Url, tls: Option<&TlsConnector>) -> Result<PubKeyResp, WalletError> {
    use std::net::TcpStream as StdTcpStream;

    let host = url
        .host_str()
        .ok_or_else(|| WalletError::Failure("missing host".into()))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addr = format!("{host}:{port}");
    let timeout = Duration::from_secs(5);
    let mut addrs = addr
        .to_socket_addrs()
        .map_err(|err| WalletError::Failure(err.to_string()))?;
    let socket = addrs
        .next()
        .ok_or_else(|| WalletError::Failure("no socket addresses".into()))?;
    let stream = StdTcpStream::connect_timeout(&socket, timeout)
        .map_err(|err| WalletError::Failure(err.to_string()))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let connector = if let Some(conn) = tls {
        conn.clone()
    } else {
        TlsConnector::builder()
            .build()
            .map_err(|err| WalletError::Failure(err.to_string()))?
    };

    let mut tls_stream = match connector.connect(host, stream) {
        Ok(stream) => stream,
        Err(HandshakeError::WouldBlock(mut mid)) => loop {
            match mid.handshake() {
                Ok(stream) => break stream,
                Err(HandshakeError::WouldBlock(next)) => {
                    mid = next;
                    thread::sleep(Duration::from_millis(5));
                }
                Err(HandshakeError::Failure(err)) => {
                    return Err(WalletError::Failure(err.to_string()));
                }
            }
        },
        Err(HandshakeError::Failure(err)) => {
            return Err(WalletError::Failure(err.to_string()));
        }
    };

    let mut path = url.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }

    let host_header = if port == 443 {
        host.to_string()
    } else {
        format!("{host}:{port}")
    };
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\nAccept: application/json\r\n\r\n"
    );
    tls_stream
        .write_all(request.as_bytes())
        .map_err(|err| WalletError::Failure(err.to_string()))?;

    let mut response = Vec::new();
    let mut header_end = None;
    let mut buf = [0u8; 1024];
    while header_end.is_none() {
        let read = tls_stream
            .read(&mut buf)
            .map_err(|err| map_tls_error(err))?;
        if read == 0 {
            return Err(WalletError::Failure("unexpected eof in headers".into()));
        }
        response.extend_from_slice(&buf[..read]);
        if response.len() > 64 * 1024 {
            return Err(WalletError::Failure("header too large".into()));
        }
        if let Some(pos) = response.windows(4).position(|w| w == b"\r\n\r\n") {
            header_end = Some(pos + 4);
        }
    }
    let header_end = header_end.expect("header_end captured");
    let headers = String::from_utf8(response[..header_end].to_vec())
        .map_err(|_| WalletError::Failure("invalid utf8 in headers".into()))?;
    if !headers.starts_with("HTTP/1.1 200") {
        return Err(WalletError::Failure("unexpected status".into()));
    }
    let content_length = headers
        .lines()
        .find_map(|line| {
            let line = line.trim();
            if line.to_ascii_lowercase().starts_with("content-length:") {
                line.split_once(':')
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            } else {
                None
            }
        })
        .ok_or_else(|| WalletError::Failure("missing content-length".into()))?;

    let mut body = response[header_end..].to_vec();
    while body.len() < content_length {
        let read = tls_stream
            .read(&mut buf)
            .map_err(|err| map_tls_error(err))?;
        if read == 0 {
            return Err(WalletError::Failure("unexpected eof in body".into()));
        }
        body.extend_from_slice(&buf[..read]);
    }
    body.truncate(content_length);
    let body_text = String::from_utf8(body).map_err(|err| WalletError::Failure(err.to_string()))?;
    serde_json::from_str(&body_text).map_err(|err| WalletError::Failure(err.to_string()))
}

fn map_tls_error(err: io::Error) -> WalletError {
    match err.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => WalletError::Timeout,
        _ => WalletError::Failure(err.to_string()),
    }
}

impl WalletSigner for RemoteSigner {
    fn public_key(&self) -> VerifyingKey {
        self.pubkeys[0].clone()
    }

    fn public_keys(&self) -> Vec<VerifyingKey> {
        self.pubkeys.clone()
    }

    fn sign(&self, msg: &[u8]) -> Result<Signature, WalletError> {
        self.sign_multisig(msg)
            .map(|mut approvals| approvals.remove(0).1)
    }

    fn sign_multisig(&self, msg: &[u8]) -> Result<Vec<(VerifyingKey, Signature)>, WalletError> {
        increment_counter!("remote_signer_request_total");
        let start = Instant::now();
        let tagged = remote_tag(msg);
        let msg_hex = hex::encode(&tagged);
        let trace_id = Uuid::new_v4();
        let payload = SignReq {
            trace: &trace_id.to_string(),
            msg: msg_hex,
        };
        let mut approvals: Vec<(VerifyingKey, Signature)> = Vec::new();
        let mut last_error: Option<WalletError> = None;
        for (i, ep) in self.endpoints.iter().enumerate() {
            let res = if ep.starts_with("ws") {
                self.sign_ws(ep, &payload)
            } else {
                self.sign_http(ep, &payload)
            };
            match res {
                Ok(sig) => {
                    if self.pubkeys[i].verify(&tagged, &sig).is_ok() {
                        approvals.push((self.pubkeys[i].clone(), sig));
                    } else {
                        warn!(%trace_id, "invalid signature");
                        last_error = Some(WalletError::Failure("invalid signature".into()));
                    }
                }
                Err(e) => {
                    warn!(%trace_id, error=%e, "signer failure");
                    last_error = Some(e);
                }
            }
            if approvals.len() >= self.threshold {
                break;
            }
        }
        if approvals.len() < self.threshold {
            return Err(
                last_error.unwrap_or_else(|| WalletError::Failure("threshold not met".into()))
            );
        }
        let mut lat = start.elapsed().as_secs_f64();
        if std::env::var("DIFF_PRIV").ok().as_deref() == Some("1") {
            let mut rng = rand::thread_rng();
            lat += rng.gen_range(-0.5..0.5);
        }
        histogram!("remote_signer_latency_seconds", lat);
        increment_counter!("remote_signer_success_total");
        Ok(approvals)
    }
}
