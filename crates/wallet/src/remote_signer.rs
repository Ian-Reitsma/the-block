use crate::{WalletError, WalletSigner};
use base64_fp::encode_standard;
use crypto_suite::{
    hashing::sha1,
    signatures::ed25519::{Signature, VerifyingKey, SIGNATURE_LENGTH},
};
use diagnostics::tracing::{info, warn};
use foundation_lazy::sync::Lazy;
use foundation_serialization::json;
use foundation_serialization::{Deserialize, Serialize};
use httpd::{
    join_path, BlockingClient, ClientConfig, ClientTlsStream, Method, TlsConnector,
    TlsConnectorError, Uri,
};
use ledger::crypto::remote_tag;
use rand::{Rng, RngCore};
use std::collections::HashMap;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{TcpStream as StdTcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

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
const CONNECT_RETRY_LIMIT: usize = 30;
const CONNECT_RETRY_INITIAL_DELAY_MS: u64 = 20;
const CONNECT_RETRY_MAX_DELAY_MS: u64 = 1_000;

struct Frame {
    opcode: u8,
    fin: bool,
    payload: Vec<u8>,
}

fn generate_trace_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

enum StreamKind {
    Plain(std::net::TcpStream),
    Tls(ClientTlsStream),
}

#[cfg(test)]
mod tests {
    use super::generate_trace_id;
    use std::collections::HashSet;

    #[test]
    fn trace_ids_follow_uuid_layout_and_remain_unique() {
        const SAMPLES: usize = 1024;
        let mut seen = HashSet::with_capacity(SAMPLES);

        for _ in 0..SAMPLES {
            let trace_id = generate_trace_id();
            assert_eq!(trace_id.len(), 36, "unexpected trace id length: {trace_id}");
            let mut chars = trace_id.chars();

            // Validate hyphen placement (8-4-4-4-12) and hexadecimal segments.
            for (idx, expected_len) in [8usize, 4, 4, 4, 12].into_iter().enumerate() {
                if idx > 0 {
                    assert_eq!(chars.next(), Some('-'));
                }
                for _ in 0..expected_len {
                    let ch = chars
                        .next()
                        .expect("trace id ended prematurely while validating segment");
                    let is_hex_digit = ch.is_ascii_digit() || matches!(ch, 'a'..='f');
                    assert!(
                        is_hex_digit,
                        "trace id segment must be lowercase hex, found '{ch}' in {trace_id}"
                    );
                }
            }
            assert!(chars.next().is_none(), "extra characters found in trace id");

            // Version nibble should encode UUID version 4 and variant bits should match RFC4122.
            assert_eq!(trace_id.chars().nth(14), Some('4'));
            assert!(matches!(
                trace_id.chars().nth(19),
                Some('8' | '9' | 'a' | 'b')
            ));

            assert!(seen.insert(trace_id), "duplicate trace id generated");
        }
    }
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

    fn tls(stream: ClientTlsStream) -> Self {
        Self {
            stream: StreamKind::Tls(stream),
        }
    }

    fn handshake(&mut self, host: &str, path: &str) -> io::Result<()> {
        let mut key_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut key_bytes);
        let key = encode_standard(&key_bytes);
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        self.stream.write_all(request.as_bytes())?;
        self.read_handshake_response(&key)
    }

    fn read_handshake_response(&mut self, key: &str) -> io::Result<()> {
        let expected_accept = handshake_accept(key)?;
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

fn handshake_accept(key: &str) -> io::Result<String> {
    let mut data = key.as_bytes().to_vec();
    data.extend_from_slice(WS_GUID.as_bytes());
    sha1::hash(&data)
        .map(|digest| encode_standard(&digest))
        .map_err(|err| io::Error::new(ErrorKind::Other, err))
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
        let endpoint_uri = Uri::parse(endpoint)
            .map_err(|err| WalletError::Failure(format!("invalid signer url: {err}")))?;
        let (scheme, use_https) = match endpoint_uri.scheme() {
            "ws" => ("http", false),
            "wss" => ("https", true),
            "http" => ("http", false),
            "https" => ("https", true),
            other => {
                return Err(WalletError::Failure(format!(
                    "unsupported signer scheme: {other}"
                )));
            }
        };
        let authority = endpoint_uri
            .authority()
            .ok_or_else(|| WalletError::Failure("missing signer host".into()))?;
        let pubkey_path = join_path(endpoint_uri.path(), "pubkey");
        let pubkey_url = format!("{scheme}://{authority}{pubkey_path}");
        let pubkey_uri =
            Uri::parse(&pubkey_url).map_err(|err| WalletError::Failure(err.to_string()))?;
        let pk: PubKeyResp = if use_https {
            fetch_pubkey_https(&pubkey_uri, tls)?
        } else {
            let resp = client
                .request(Method::Get, &pubkey_url)
                .map_err(|e| WalletError::Failure(e.to_string()))?
                .timeout(Duration::from_secs(5))
                .send()
                .map_err(|e| WalletError::Failure(e.to_string()))?;
            resp.json()
                .map_err(|e| WalletError::Failure(e.to_string()))?
        };
        let bytes = crypto_suite::hex::decode(pk.pubkey)
            .map_err(|e| WalletError::Failure(e.to_string()))?;
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
        foundation_metrics::increment_counter!("remote_signer_key_rotation_total");
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
        let mut client_config = ClientConfig::from_env(&["REMOTE_SIGNER_TLS", "TB_HTTP_TLS"])
            .unwrap_or_else(|err| {
                eprintln!(
                "wallet remote signer: falling back to default HTTP client after TLS error: {err}"
            );
                ClientConfig::default()
            });
        let mut tls = None;
        if let (Ok(cert_path), Ok(key_path)) = (
            std::env::var("REMOTE_SIGNER_TLS_CERT"),
            std::env::var("REMOTE_SIGNER_TLS_KEY"),
        ) {
            let mut tls_builder = TlsConnector::builder();
            tls_builder
                .identity_from_files(&cert_path, &key_path)
                .map_err(|e| WalletError::Failure(e.to_string()))?;
            let mut has_anchor = false;
            if let Ok(ca_path) = std::env::var("REMOTE_SIGNER_TLS_CA") {
                tls_builder
                    .add_trust_anchor_from_file(&ca_path)
                    .map_err(|e| WalletError::Failure(e.to_string()))?;
                has_anchor = true;
            }
            tls_builder.danger_accept_invalid_certs(!has_anchor);
            let connector = tls_builder
                .build()
                .map_err(|e| WalletError::Failure(e.to_string()))?;
            client_config = client_config.with_tls_connector(connector.clone());
            tls = Some(connector);
        }
        let client = BlockingClient::new(client_config);
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
                        let sig_bytes = crypto_suite::hex::decode(r.sig)
                            .map_err(|e| WalletError::Failure(e.to_string()))?;
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
                    let is_timeout = is_timeout_error(&e);
                    if attempt == self.retries {
                        if is_timeout {
                            return Err(WalletError::Timeout);
                        }
                        return Err(WalletError::Failure(e.to_string()));
                    }
                    warn!(%payload.trace, error=%e, "retrying signer request");
                    if is_timeout {
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        }
        Err(WalletError::Failure("unreachable".into()))
    }

    fn sign_ws(&self, endpoint: &str, payload: &SignReq) -> Result<Signature, WalletError> {
        let url = format!("{endpoint}/sign");
        let url = Uri::parse(&url).map_err(|e| WalletError::Failure(e.to_string()))?;
        let host = url
            .host_str()
            .ok_or_else(|| WalletError::Failure("missing host".into()))?;
        let addr = url
            .socket_addr()
            .ok_or_else(|| WalletError::Failure("missing address".into()))?;
        let tcp_factory = || connect_with_retry(|| StdTcpStream::connect(&addr));
        let mut ws = if let Some(connector) = &self.tls {
            let tls_stream = tls_connect_with_retry(connector, host, tcp_factory)
                .map_err(|e| WalletError::Failure(e.to_string()))?;
            BlockingWebSocket::tls(tls_stream)
        } else {
            let tcp = tcp_factory().map_err(|e| WalletError::Failure(e.to_string()))?;
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
        let host_header = url.host_header().unwrap_or_else(|| host.to_string());

        ws.handshake(&host_header, &path)
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        ws.send_text(&json::to_string(payload).expect("serialize sign payload"))
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let txt = ws
            .read_text()
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let r: SignResp = json::from_str(&txt).map_err(|e| WalletError::Failure(e.to_string()))?;
        let sig_bytes =
            crypto_suite::hex::decode(r.sig).map_err(|e| WalletError::Failure(e.to_string()))?;
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

fn fetch_pubkey_https(url: &Uri, tls: Option<&TlsConnector>) -> Result<PubKeyResp, WalletError> {
    use std::net::TcpStream as StdTcpStream;

    let host = url
        .host_str()
        .ok_or_else(|| WalletError::Failure("missing host".into()))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addr = url
        .socket_addr()
        .ok_or_else(|| WalletError::Failure("no socket addresses".into()))?;
    let timeout = Duration::from_secs(5);
    let mut addrs = addr
        .to_socket_addrs()
        .map_err(|err| WalletError::Failure(err.to_string()))?;
    let socket = addrs
        .next()
        .ok_or_else(|| WalletError::Failure("no socket addresses".into()))?;
    let connector = if let Some(conn) = tls {
        conn.clone()
    } else {
        let mut builder = TlsConnector::builder();
        builder.danger_accept_invalid_certs(true);
        builder
            .build()
            .map_err(|err| WalletError::Failure(err.to_string()))?
    };

    let mut tls_stream = tls_connect_with_retry(&connector, host, || {
        let stream = connect_with_retry(|| StdTcpStream::connect_timeout(&socket, timeout))?;
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        Ok(stream)
    })
    .map_err(|err| WalletError::Failure(err.to_string()))?;

    let mut path = url.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }

    let host_header = if port == 443 {
        if url.host_is_ipv6() {
            format!("[{host}]")
        } else {
            host.to_string()
        }
    } else {
        url.host_header().unwrap_or_else(|| host.to_string())
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
    json::from_str(&body_text).map_err(|err| WalletError::Failure(err.to_string()))
}

fn connect_with_retry<F>(mut attempt: F) -> io::Result<StdTcpStream>
where
    F: FnMut() -> io::Result<StdTcpStream>,
{
    let mut delay = Duration::from_millis(CONNECT_RETRY_INITIAL_DELAY_MS);
    for remaining in 0..CONNECT_RETRY_LIMIT {
        match attempt() {
            Ok(stream) => return Ok(stream),
            Err(err) if should_retry_connect(&err) && remaining + 1 < CONNECT_RETRY_LIMIT => {
                warn!(error=?err, "tcp connect retry");
                thread::sleep(delay);
                delay = (delay + delay).min(Duration::from_millis(CONNECT_RETRY_MAX_DELAY_MS));
            }
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        ErrorKind::Other,
        "connect retries exhausted",
    ))
}

fn should_retry_connect(err: &io::Error) -> bool {
    if err.kind() == ErrorKind::WouldBlock {
        return true;
    }
    match err.raw_os_error() {
        Some(code) => matches!(code, 35 | 36 | 37 | 10035 | 10036 | 10037 | 114 | 115),
        None => false,
    }
}

fn tls_connect_with_retry<F>(
    connector: &TlsConnector,
    host: &str,
    mut stream_factory: F,
) -> Result<ClientTlsStream, TlsConnectorError>
where
    F: FnMut() -> io::Result<StdTcpStream>,
{
    let mut delay = Duration::from_millis(CONNECT_RETRY_INITIAL_DELAY_MS);
    for attempt in 0..CONNECT_RETRY_LIMIT {
        let stream = match stream_factory() {
            Ok(stream) => stream,
            Err(err) if should_retry_connect(&err) && attempt + 1 < CONNECT_RETRY_LIMIT => {
                warn!(error=?err, attempt, "tcp connect retry before tls handshake");
                thread::sleep(delay);
                delay = (delay + delay).min(Duration::from_millis(CONNECT_RETRY_MAX_DELAY_MS));
                continue;
            }
            Err(err) => return Err(TlsConnectorError::Io(err)),
        };
        match connector.connect(host, stream) {
            Ok(tls) => return Ok(tls),
            Err(err) if should_retry_tls(&err) && attempt + 1 < CONNECT_RETRY_LIMIT => {
                warn!(error=?err, attempt, "tls handshake retry");
                thread::sleep(delay);
                delay = (delay + delay).min(Duration::from_millis(CONNECT_RETRY_MAX_DELAY_MS));
            }
            Err(err) => return Err(err),
        }
    }
    Err(TlsConnectorError::Io(io::Error::new(
        ErrorKind::Other,
        "tls connect retries exhausted",
    )))
}

fn should_retry_tls(err: &TlsConnectorError) -> bool {
    match err {
        TlsConnectorError::Io(io_err) => should_retry_connect(io_err),
        _ => false,
    }
}

fn map_tls_error(err: io::Error) -> WalletError {
    match err.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => WalletError::Timeout,
        _ => WalletError::Failure(err.to_string()),
    }
}

fn is_timeout_error(err: &httpd::ClientError) -> bool {
    if err.is_timeout() {
        return true;
    }
    if let httpd::ClientError::Io(io_err) = err {
        return matches!(
            io_err.kind(),
            ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
        ) || matches!(
            io_err.raw_os_error(),
            Some(11)   // EAGAIN on Linux
                | Some(35)  // EWOULDBLOCK on macOS
                | Some(36)
                | Some(37)
                | Some(60)  // ETIMEDOUT on macOS
                | Some(10035) // WSAEWOULDBLOCK
                | Some(10036)
                | Some(10037)
                | Some(110) // ETIMEDOUT on many Unixes
                | Some(114)
                | Some(115)
        );
    }
    false
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
        foundation_metrics::increment_counter!("remote_signer_request_total");
        let start = Instant::now();
        let tagged = remote_tag(msg);
        let msg_hex = crypto_suite::hex::encode(&tagged);
        let trace_id = generate_trace_id();
        let payload = SignReq {
            trace: &trace_id,
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
        foundation_metrics::histogram!("remote_signer_latency_seconds", lat);
        foundation_metrics::increment_counter!("remote_signer_success_total");
        Ok(approvals)
    }
}
