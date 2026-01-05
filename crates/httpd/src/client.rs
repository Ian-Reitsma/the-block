use crate::{Method, StatusCode, TlsConnector, TlsConnectorError, Uri};
use foundation_serialization::de::DeserializeOwned;
use foundation_serialization::{Error as SerializationError, Serialize, json};
use runtime::io::BufferedTcpStream;
use runtime::net::TcpStream;
use runtime::spawn_blocking;
use runtime::timeout;
use std::collections::HashMap;
use std::fmt;
use std::io;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::ToSocketAddrs;
use std::string::FromUtf8Error;
use std::thread;
use std::time::Duration;

/// Configuration toggles applied to outbound HTTP requests.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Maximum duration allowed for establishing a TCP connection.
    pub connect_timeout: Duration,
    /// Maximum duration allowed for completing the HTTP exchange.
    pub request_timeout: Duration,
    /// Optional upper bound for reading the response payload.
    pub read_timeout: Option<Duration>,
    /// Maximum duration allowed for completing the TLS handshake.
    pub tls_handshake_timeout: Duration,
    /// Maximum number of response bytes buffered in memory.
    pub max_response_bytes: usize,
    /// Optional TLS connector used for HTTPS requests.
    pub tls: Option<TlsConnector>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        let tls_handshake_timeout = Duration::from_secs(10);
        Self {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(15),
            read_timeout: Some(Duration::from_secs(15)),
            tls_handshake_timeout,
            max_response_bytes: 16 * 1024 * 1024,
            tls: super::default_tls_connector()
                .map(|connector| connector.with_handshake_timeout(tls_handshake_timeout)),
        }
    }
}

impl ClientConfig {
    /// Attach a TLS connector to the configuration.
    pub fn with_tls_connector(mut self, connector: TlsConnector) -> Self {
        self.tls = Some(connector);
        self
    }

    /// Construct a configuration that pulls TLS settings from the provided
    /// environment variable prefixes.
    pub fn from_env(prefixes: &[&str]) -> Result<Self, TlsConnectorError> {
        let mut config = Self::default();
        if let Some(connector) = super::tls_connector_from_env_any(prefixes)? {
            config.tls = Some(connector.with_handshake_timeout(config.tls_handshake_timeout));
        }
        Ok(config)
    }

    /// Override the TLS handshake timeout.
    pub fn with_tls_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.tls_handshake_timeout = timeout;
        if let Some(connector) = self.tls.take() {
            self.tls = Some(connector.with_handshake_timeout(timeout));
        }
        self
    }
}

/// Simple HTTP/1.1 client built on top of the runtime socket primitives.
#[derive(Debug, Clone)]
pub struct Client {
    config: ClientConfig,
}

fn http_debug_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("TB_HTTP_DEBUG").is_ok())
}

fn force_blocking_http() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("TB_HTTP_FORCE_BLOCKING").is_ok())
}

impl Client {
    /// Create a client with the provided configuration.
    pub fn new(config: ClientConfig) -> Self {
        let mut config = config;
        if let Some(connector) = config.tls.take() {
            config.tls = Some(connector.with_handshake_timeout(config.tls_handshake_timeout));
        }
        Self { config }
    }

    /// Create a client using the default configuration knobs.
    pub fn default() -> Self {
        Self::new(ClientConfig::default())
    }

    /// Create a client using TLS settings sourced from the environment.
    pub fn with_tls_from_env(prefixes: &[&str]) -> Result<Self, TlsConnectorError> {
        let config = ClientConfig::from_env(prefixes)?;
        Ok(Self::new(config))
    }

    /// Prepare an outbound request to the provided URL.
    pub fn request(&self, method: Method, url: &str) -> Result<RequestBuilder<'_>, ClientError> {
        let parsed = Uri::parse(url).map_err(|err| ClientError::InvalidUrl(err.to_string()))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(ClientError::UnsupportedScheme(parsed.scheme().to_string()));
        }
        Ok(RequestBuilder {
            client: self,
            method,
            url: parsed,
            headers: HashMap::new(),
            body: Vec::new(),
            timeout: None,
        })
    }
}

/// Builder used to stage outbound HTTP requests prior to execution.
pub struct RequestBuilder<'a> {
    client: &'a Client,
    method: Method,
    url: Uri,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout: Option<Duration>,
}

impl<'a> RequestBuilder<'a> {
    /// Attach a header to the outbound request. Header names are normalized to
    /// lowercase per the in-house server implementation.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(name.into().to_ascii_lowercase(), value.into());
        self
    }

    /// Provide a binary request body. Callers are expected to set the
    /// appropriate `content-type` header when sending structured payloads.
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    /// Convenience helper that serializes `value` using the canonical JSON
    /// codec and sets the appropriate content-type header.
    pub fn json<T: Serialize>(mut self, value: &T) -> Result<Self, ClientError> {
        self.body = json::to_vec(value)?;
        self.headers
            .insert("content-type".into(), "application/json".into());
        Ok(self)
    }

    /// Override the request timeout for this invocation.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Execute the prepared request and return the HTTP response.
    pub async fn send(self) -> Result<ClientResponse, ClientError> {
        execute(
            self.client,
            self.method,
            self.url,
            self.headers,
            self.body,
            self.timeout,
        )
        .await
    }
}

/// HTTP client error surface mirroring the failure modes exposed by the server
/// utilities.
#[derive(Debug)]
pub enum ClientError {
    Io(io::Error),
    Timeout,
    InvalidUrl(String),
    UnsupportedScheme(String),
    InvalidResponse(&'static str),
    ResponseTooLarge,
    Serialization(SerializationError),
    Utf8(FromUtf8Error),
    Tls(TlsConnectorError),
}

impl From<io::Error> for ClientError {
    fn from(value: io::Error) -> Self {
        ClientError::Io(value)
    }
}

impl From<SerializationError> for ClientError {
    fn from(value: SerializationError) -> Self {
        ClientError::Serialization(value)
    }
}

impl From<FromUtf8Error> for ClientError {
    fn from(value: FromUtf8Error) -> Self {
        ClientError::Utf8(value)
    }
}

impl From<TlsConnectorError> for ClientError {
    fn from(value: TlsConnectorError) -> Self {
        ClientError::Tls(value)
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Io(err) => write!(f, "io error: {err}"),
            ClientError::Timeout => write!(f, "timeout"),
            ClientError::InvalidUrl(url) => write!(f, "invalid url: {url}"),
            ClientError::UnsupportedScheme(scheme) => {
                write!(f, "unsupported url scheme: {scheme}")
            }
            ClientError::InvalidResponse(reason) => {
                write!(f, "malformed http response: {reason}")
            }
            ClientError::ResponseTooLarge => write!(f, "response exceeds configured limit"),
            ClientError::Serialization(err) => write!(f, "serialization error: {err}"),
            ClientError::Utf8(err) => write!(f, "utf-8 error: {err}"),
            ClientError::Tls(err) => write!(f, "tls error: {err}"),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ClientError::Io(err) => Some(err),
            ClientError::Serialization(err) => Some(err),
            ClientError::Utf8(err) => Some(err),
            ClientError::Tls(err) => Some(err),
            _ => None,
        }
    }
}

/// Structured representation of an HTTP response.
#[derive(Debug, Clone)]
pub struct ClientResponse {
    status: StatusCode,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl ClientResponse {
    /// Access the response status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Return the decoded body as a byte slice.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Retrieve a header value using a case-insensitive lookup.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|value| value.as_str())
    }

    /// Deserialize the payload using the canonical JSON codec.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, ClientError> {
        json::from_slice(&self.body).map_err(ClientError::from)
    }

    /// Interpret the payload as UTF-8 text, returning an owned string.
    pub fn text(&self) -> Result<String, ClientError> {
        String::from_utf8(self.body.clone()).map_err(ClientError::from)
    }

    /// Consume the response and return the owned body bytes.
    pub fn into_body(self) -> Vec<u8> {
        self.body
    }
}

impl ClientError {
    /// Returns true when the error originated from a timeout.
    pub fn is_timeout(&self) -> bool {
        matches!(self, ClientError::Timeout)
    }
}

async fn execute(
    client: &Client,
    method: Method,
    url: Uri,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout_override: Option<Duration>,
) -> Result<ClientResponse, ClientError> {
    match url.scheme() {
        "http" => {
            if force_blocking_http() {
                return execute_http_blocking(client, method, url, headers, body, timeout_override)
                    .await;
            }

            // Clone upfront to allow a fallback to the blocking path if the async connect
            // times out (observed intermittently on some Linux environments).
            let retry_url = url.clone();
            let retry_headers = headers.clone();
            let retry_body = body.clone();

            match execute_http_async(client, method, url, headers, body, timeout_override).await {
                Ok(response) => Ok(response),
                Err(ClientError::Timeout) => {
                    if http_debug_enabled() {
                        eprintln!(
                            "[http-client] async path timed out, falling back to blocking connect"
                        );
                    }
                    execute_http_blocking(
                        client,
                        method,
                        retry_url,
                        retry_headers,
                        retry_body,
                        timeout_override,
                    )
                    .await
                }
                Err(err) => Err(err),
            }
        }
        "https" => {
            let connector = client
                .config
                .tls
                .clone()
                .ok_or_else(|| ClientError::UnsupportedScheme("https".into()))?;
            execute_https(
                client,
                method,
                url,
                headers,
                body,
                timeout_override,
                connector,
            )
            .await
        }
        scheme => Err(ClientError::UnsupportedScheme(scheme.to_string())),
    }
}

fn resolve_addr(target: &str) -> Option<std::net::SocketAddr> {
    target.to_socket_addrs().ok()?.next()
}

fn request_target(url: &Uri) -> String {
    let mut path = url.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    path
}

async fn read_response(
    stream: &mut BufferedTcpStream,
    max_body: usize,
) -> Result<ClientResponse, ClientError> {
    let mut status_line = String::new();
    let read = stream
        .read_line(&mut status_line)
        .await
        .map_err(ClientError::from)?;
    if read == 0 {
        return Err(ClientError::InvalidResponse("empty response"));
    }
    let status = parse_status_line(&status_line)?;
    let headers = read_headers(stream).await?;
    let body = read_body(stream, &headers, max_body).await?;
    Ok(ClientResponse {
        status,
        headers,
        body,
    })
}

fn parse_status_line(line: &str) -> Result<StatusCode, ClientError> {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    let mut parts = trimmed.splitn(3, ' ');
    let version = parts
        .next()
        .ok_or(ClientError::InvalidResponse("missing version"))?;
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(ClientError::InvalidResponse("unsupported http version"));
    }
    let status = parts
        .next()
        .ok_or(ClientError::InvalidResponse("missing status"))?
        .parse::<u16>()
        .map_err(|_| ClientError::InvalidResponse("invalid status code"))?;
    Ok(StatusCode(status))
}

async fn read_headers(
    stream: &mut BufferedTcpStream,
) -> Result<HashMap<String, String>, ClientError> {
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        let read = stream
            .read_line(&mut line)
            .await
            .map_err(ClientError::from)?;
        if read == 0 {
            return Err(ClientError::InvalidResponse(
                "unexpected eof reading headers",
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        } else {
            return Err(ClientError::InvalidResponse("malformed header"));
        }
    }
    Ok(headers)
}

async fn read_body(
    stream: &mut BufferedTcpStream,
    headers: &HashMap<String, String>,
    max_body: usize,
) -> Result<Vec<u8>, ClientError> {
    if let Some(te) = headers.get("transfer-encoding") {
        if te.eq_ignore_ascii_case("chunked") {
            return read_chunked_body(stream, max_body).await;
        }
    }
    let length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if length > max_body {
        return Err(ClientError::ResponseTooLarge);
    }
    let mut body = vec![0u8; length];
    if length > 0 {
        stream
            .read_exact(&mut body)
            .await
            .map_err(ClientError::from)?;
    }
    Ok(body)
}

async fn read_chunked_body(
    stream: &mut BufferedTcpStream,
    max_body: usize,
) -> Result<Vec<u8>, ClientError> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        let read = stream
            .read_line(&mut size_line)
            .await
            .map_err(ClientError::from)?;
        if read == 0 {
            return Err(ClientError::InvalidResponse("unexpected eof reading chunk"));
        }
        let trimmed = size_line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }
        let size_str = trimmed
            .split(';')
            .next()
            .ok_or(ClientError::InvalidResponse("invalid chunk size"))?;
        let size = usize::from_str_radix(size_str, 16)
            .map_err(|_| ClientError::InvalidResponse("invalid chunk size"))?;
        if size == 0 {
            // Consume the trailing header terminator after the final chunk.
            loop {
                let mut trailer = String::new();
                let read = stream
                    .read_line(&mut trailer)
                    .await
                    .map_err(ClientError::from)?;
                if read == 0 || trailer == "\r\n" || trailer == "\n" {
                    break;
                }
            }
            break;
        }
        if body.len() + size > max_body {
            return Err(ClientError::ResponseTooLarge);
        }
        let mut chunk = vec![0u8; size];
        stream
            .read_exact(&mut chunk)
            .await
            .map_err(ClientError::from)?;
        body.extend_from_slice(&chunk);
        // Discard the trailing CRLF after each chunk.
        let mut crlf = [0u8; 2];
        stream
            .read_exact(&mut crlf)
            .await
            .map_err(ClientError::from)?;
        if &crlf != b"\r\n" {
            return Err(ClientError::InvalidResponse("chunk missing terminator"));
        }
    }
    Ok(body)
}

async fn execute_http_async(
    client: &Client,
    method: Method,
    url: Uri,
    mut headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout_override: Option<Duration>,
) -> Result<ClientResponse, ClientError> {
    let debug = http_debug_enabled();
    let host = url
        .host_str()
        .ok_or_else(|| ClientError::InvalidResponse("missing host"))?;
    let socket = url
        .socket_addr()
        .ok_or_else(|| ClientError::InvalidResponse("unresolvable host"))?;
    let addr =
        resolve_addr(&socket).ok_or_else(|| ClientError::InvalidResponse("unresolvable host"))?;
    let connect_timeout = client.config.connect_timeout;
    let request_timeout = timeout_override.unwrap_or(client.config.request_timeout);
    let stream = timeout(connect_timeout, TcpStream::connect(addr))
        .await
        .map_err(|_| ClientError::Timeout)??;
    if debug {
        eprintln!("[http-client] connected to {addr}");
    }
    let mut buffered = BufferedTcpStream::new(stream);

    let request = build_request(method, &url, &mut headers, &body, host)?;

    timeout(request_timeout, buffered.write_all(request.as_bytes()))
        .await
        .map_err(|_| ClientError::Timeout)??;
    if debug {
        eprintln!(
            "[http-client] wrote request headers ({} bytes)",
            request.len()
        );
    }
    if !body.is_empty() {
        timeout(request_timeout, buffered.write_all(&body))
            .await
            .map_err(|_| ClientError::Timeout)??;
        if debug {
            eprintln!("[http-client] wrote request body ({} bytes)", body.len());
        }
    }
    timeout(request_timeout, buffered.get_mut().flush())
        .await
        .map_err(|_| ClientError::Timeout)??;
    if debug {
        eprintln!("[http-client] flushed request");
    }

    // Use per-request timeout if set, then configured read_timeout, then request_timeout
    // This ensures per-request timeouts (via .timeout()) apply to the entire request-response cycle
    let read_limit = timeout_override
        .or(client.config.read_timeout)
        .unwrap_or(request_timeout);
    let response = timeout(
        read_limit,
        read_response(&mut buffered, client.config.max_response_bytes),
    )
    .await
    .map_err(|_| ClientError::Timeout)??;
    if debug {
        eprintln!("[http-client] received response status {}", response.status);
    }
    Ok(response)
}

async fn execute_http_blocking(
    client: &Client,
    method: Method,
    url: Uri,
    mut headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout_override: Option<Duration>,
) -> Result<ClientResponse, ClientError> {
    let config = client.config.clone();
    let debug = http_debug_enabled();
    spawn_blocking(move || {
        let host = url
            .host_str()
            .ok_or_else(|| ClientError::InvalidResponse("missing host"))?;
        let socket = url
            .socket_addr()
            .ok_or_else(|| ClientError::InvalidResponse("unresolvable host"))?;
        let addr =
            resolve_addr(&socket).ok_or_else(|| ClientError::InvalidResponse("unresolvable host"))?;
        let connect_timeout = config.connect_timeout;
        let request_timeout = timeout_override.unwrap_or(config.request_timeout);
        let read_limit = timeout_override
            .or(config.read_timeout)
            .unwrap_or(request_timeout);

        let mut stream = connect_blocking_with_retry(&addr, connect_timeout)
            .map_err(ClientError::from)?;
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(read_limit));
        let _ = stream.set_write_timeout(Some(request_timeout));

        if debug {
            eprintln!("[http-client] [blocking] connected to {addr}");
        }

        let request = build_request(method, &url, &mut headers, &body, host)?;
        stream.write_all(request.as_bytes())?;
        if debug {
            eprintln!(
                "[http-client] [blocking] wrote request headers ({} bytes)",
                request.len()
            );
        }
        if !body.is_empty() {
            stream.write_all(&body)?;
            if debug {
                eprintln!(
                    "[http-client] [blocking] wrote request body ({} bytes)",
                    body.len()
                );
            }
        }
        stream.flush()?;
        if debug {
            eprintln!("[http-client] [blocking] flushed request");
        }

        let mut reader = BufReader::new(stream);
        let response = read_response_blocking(&mut reader, config.max_response_bytes)?;
        if debug {
            eprintln!(
                "[http-client] [blocking] received response status {}",
                response.status
            );
        }
        Ok(response)
    })
    .await
    .map_err(|err| {
        ClientError::Io(io::Error::new(
            io::ErrorKind::Other,
            format!("join blocking http task: {err}"),
        ))
    })?
}

async fn execute_https(
    client: &Client,
    method: Method,
    url: Uri,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout_override: Option<Duration>,
    connector: TlsConnector,
) -> Result<ClientResponse, ClientError> {
    let host = url
        .host_str()
        .ok_or_else(|| ClientError::InvalidResponse("missing host"))?
        .to_string();
    let socket = url
        .socket_addr()
        .ok_or_else(|| ClientError::InvalidResponse("unresolvable host"))?;
    let addr =
        resolve_addr(&socket).ok_or_else(|| ClientError::InvalidResponse("unresolvable host"))?;
    let connect_timeout = client.config.connect_timeout;
    let request_timeout = timeout_override.unwrap_or(client.config.request_timeout);
    let max_response_bytes = client.config.max_response_bytes;
    let url_clone = url.clone();

    let read_timeout = client.config.read_timeout;
    let join = spawn_blocking(move || {
        execute_https_blocking(
            method,
            url_clone,
            headers,
            body,
            connector,
            host,
            addr,
            connect_timeout,
            request_timeout,
            max_response_bytes,
            read_timeout,
        )
    })
    .await
    .map_err(|err| {
        ClientError::Io(io::Error::new(
            io::ErrorKind::Other,
            format!("join blocking https task: {err}"),
        ))
    })?;

    join
}

fn execute_https_blocking(
    method: Method,
    url: Uri,
    mut headers: HashMap<String, String>,
    body: Vec<u8>,
    connector: TlsConnector,
    host: String,
    addr: std::net::SocketAddr,
    connect_timeout: Duration,
    request_timeout: Duration,
    max_response_bytes: usize,
    read_timeout: Option<Duration>,
) -> Result<ClientResponse, ClientError> {
    let debug = std::env::var("TB_TLS_TEST_DEBUG").is_ok();
    let stream = connect_blocking_with_retry(&addr, connect_timeout)?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(None);
    let _ = stream.set_write_timeout(None);

    if debug {
        eprintln!("[tls-client] starting https handshake to {addr}");
    }
    let mut tls_stream = connector.connect(&host, stream)?;
    // Use the configured read_timeout if set, otherwise fall back to request_timeout
    // This ensures per-request timeouts apply to the entire request-response cycle
    let effective_read_timeout = read_timeout.unwrap_or(request_timeout);
    let _ = tls_stream.set_read_timeout(Some(effective_read_timeout));
    let _ = tls_stream.set_write_timeout(Some(request_timeout));
    if debug {
        eprintln!("[tls-client] https handshake complete, sending request");
    }
    let request = build_request(method, &url, &mut headers, &body, &host)?;
    tls_stream.write_all(request.as_bytes())?;
    if !body.is_empty() {
        tls_stream.write_all(&body)?;
    }
    tls_stream.flush()?;
    if debug {
        eprintln!("[tls-client] request sent, reading response");
    }

    let mut reader = BufReader::new(tls_stream);
    read_response_blocking(&mut reader, max_response_bytes)
}

fn build_request(
    method: Method,
    url: &Uri,
    headers: &mut HashMap<String, String>,
    body: &[u8],
    host: impl AsRef<str>,
) -> Result<String, ClientError> {
    let path = request_target(url);
    let mut request = format!("{} {} HTTP/1.1\r\n", method.as_str(), path);
    let host_header = url
        .host_header()
        .unwrap_or_else(|| host.as_ref().to_string());
    headers.insert("host".into(), host_header);
    headers
        .entry("connection".into())
        .or_insert_with(|| "close".to_string());
    if !body.is_empty() {
        headers
            .entry("content-length".into())
            .or_insert_with(|| body.len().to_string());
    } else {
        headers
            .entry("content-length".into())
            .or_insert_with(|| "0".into());
    }
    for (name, value) in headers.iter() {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    Ok(request)
}

fn connect_blocking_with_retry(
    addr: &std::net::SocketAddr,
    timeout: Duration,
) -> io::Result<std::net::TcpStream> {
    const MAX_ATTEMPTS: usize = 6;
    let mut delay = Duration::from_millis(25);
    for attempt in 0..MAX_ATTEMPTS {
        match std::net::TcpStream::connect_timeout(addr, timeout) {
            Ok(stream) => {
                let _ = stream.set_nonblocking(false);
                return Ok(stream);
            }
            Err(err) if should_retry_connect(&err) && attempt + 1 < MAX_ATTEMPTS => {
                thread::sleep(delay);
                delay = (delay + delay).min(Duration::from_millis(300));
            }
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::Other,
        "exhausted tcp connect retries",
    ))
}

fn should_retry_connect(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::WouldBlock {
        return true;
    }
    match err.raw_os_error() {
        Some(code) => matches!(
            code,
            35 | 36 | 37 | 60 | 61 | 10035 | 10036 | 10037 | 114 | 115
        ),
        None => false,
    }
}

fn read_response_blocking<R: Read>(
    reader: &mut BufReader<R>,
    max_body: usize,
) -> Result<ClientResponse, ClientError> {
    let mut status_line = String::new();
    let read = reader.read_line(&mut status_line)?;
    if read == 0 {
        return Err(ClientError::InvalidResponse("empty response"));
    }
    let status = parse_status_line(&status_line)?;
    let headers = read_headers_blocking(reader)?;
    let body = read_body_blocking(reader, &headers, max_body)?;
    Ok(ClientResponse {
        status,
        headers,
        body,
    })
}

fn read_headers_blocking<R: Read>(
    reader: &mut BufReader<R>,
) -> Result<HashMap<String, String>, ClientError> {
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Err(ClientError::InvalidResponse(
                "unexpected eof reading headers",
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        } else {
            return Err(ClientError::InvalidResponse("malformed header"));
        }
    }
    Ok(headers)
}

fn read_body_blocking<R: Read>(
    reader: &mut BufReader<R>,
    headers: &HashMap<String, String>,
    max_body: usize,
) -> Result<Vec<u8>, ClientError> {
    if let Some(te) = headers.get("transfer-encoding") {
        if te.eq_ignore_ascii_case("chunked") {
            return read_chunked_body_blocking(reader, max_body);
        }
    }
    let length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if length > max_body {
        return Err(ClientError::ResponseTooLarge);
    }
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok(body)
}

fn read_chunked_body_blocking<R: Read>(
    reader: &mut BufReader<R>,
    max_body: usize,
) -> Result<Vec<u8>, ClientError> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        let read = reader.read_line(&mut size_line)?;
        if read == 0 {
            return Err(ClientError::InvalidResponse(
                "unexpected eof reading chunk size",
            ));
        }
        let size = size_line
            .trim_end_matches(['\r', '\n'])
            .split(';')
            .next()
            .ok_or(ClientError::InvalidResponse("missing chunk size"))?
            .trim();
        let chunk_size = usize::from_str_radix(size, 16)
            .map_err(|_| ClientError::InvalidResponse("invalid chunk size"))?;
        if chunk_size == 0 {
            loop {
                let mut line = String::new();
                let read = reader.read_line(&mut line)?;
                if read == 0 || line == "\r\n" || line == "\n" {
                    break;
                }
            }
            break;
        }
        if body.len() + chunk_size > max_body {
            return Err(ClientError::ResponseTooLarge);
        }
        let mut chunk = vec![0u8; chunk_size];
        reader.read_exact(&mut chunk)?;
        body.extend_from_slice(&chunk);
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf)?;
        if crlf != [b'\r', b'\n'] {
            return Err(ClientError::InvalidResponse("missing chunk terminator"));
        }
    }
    Ok(body)
}
