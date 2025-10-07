use crate::{JSON_CODEC, Method, StatusCode, Uri};
use codec::Codec;
use runtime::io::BufferedTcpStream;
use runtime::net::TcpStream;
use runtime::timeout;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::io;
use std::net::ToSocketAddrs;
use std::string::FromUtf8Error;
use std::time::Duration;
use thiserror::Error;

/// Configuration toggles applied to outbound HTTP requests.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Maximum duration allowed for establishing a TCP connection.
    pub connect_timeout: Duration,
    /// Maximum duration allowed for completing the HTTP exchange.
    pub request_timeout: Duration,
    /// Maximum number of response bytes buffered in memory.
    pub max_response_bytes: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(15),
            max_response_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Simple HTTP/1.1 client built on top of the runtime socket primitives.
#[derive(Debug, Clone)]
pub struct Client {
    config: ClientConfig,
}

impl Client {
    /// Create a client with the provided configuration.
    pub fn new(config: ClientConfig) -> Self {
        Self { config }
    }

    /// Create a client using the default configuration knobs.
    pub fn default() -> Self {
        Self::new(ClientConfig::default())
    }

    /// Prepare an outbound request to the provided URL.
    pub fn request(&self, method: Method, url: &str) -> Result<RequestBuilder<'_>, ClientError> {
        let parsed = Uri::parse(url).map_err(|err| ClientError::InvalidUrl(err.to_string()))?;
        if parsed.scheme() != "http" {
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
        self.body = codec::serialize(JSON_CODEC, value)?;
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
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("timeout")]
    Timeout,
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    #[error("unsupported url scheme: {0}")]
    UnsupportedScheme(String),
    #[error("malformed http response: {0}")]
    InvalidResponse(&'static str),
    #[error("response exceeds configured limit")]
    ResponseTooLarge,
    #[error("codec error: {0}")]
    Codec(#[from] codec::Error),
    #[error("utf-8 error: {0}")]
    Utf8(#[from] FromUtf8Error),
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
        codec::deserialize(JSON_CODEC, &self.body).map_err(ClientError::from)
    }

    /// Deserialize the payload using an arbitrary codec profile.
    pub fn decode<T: DeserializeOwned>(&self, codec: Codec) -> Result<T, ClientError> {
        codec::deserialize(codec, &self.body).map_err(ClientError::from)
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
    mut headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout_override: Option<Duration>,
) -> Result<ClientResponse, ClientError> {
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
    let mut buffered = BufferedTcpStream::new(stream);

    let path = request_target(&url);
    let mut request = format!("{} {} HTTP/1.1\r\n", method.as_str(), path);
    let host_header = url.host_header().unwrap_or_else(|| host.to_string());
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

    timeout(request_timeout, buffered.write_all(request.as_bytes()))
        .await
        .map_err(|_| ClientError::Timeout)??;
    if !body.is_empty() {
        timeout(request_timeout, buffered.write_all(&body))
            .await
            .map_err(|_| ClientError::Timeout)??;
    }
    timeout(request_timeout, buffered.get_mut().flush())
        .await
        .map_err(|_| ClientError::Timeout)??;

    let response = timeout(
        request_timeout,
        read_response(&mut buffered, client.config.max_response_bytes),
    )
    .await
    .map_err(|_| ClientError::Timeout)??;
    Ok(response)
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
