use codec::{self, JsonProfile};
use runtime::io::BufferedTcpStream;
use runtime::net::{TcpListener, TcpStream};
use runtime::{spawn, timeout};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use url::form_urlencoded;

pub mod blocking;
pub mod client;
pub mod jsonrpc;
pub use blocking::{BlockingClient, BlockingRequestBuilder};
pub use client::{Client as HttpClient, ClientConfig, ClientError, ClientResponse};
pub use jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcRouter};

pub(crate) const JSON_CODEC: codec::Codec = codec::Codec::Json(JsonProfile::Canonical);

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
    pub const BAD_REQUEST: StatusCode = StatusCode(400);
    pub const UNAUTHORIZED: StatusCode = StatusCode(401);
    pub const FORBIDDEN: StatusCode = StatusCode(403);
    pub const NOT_FOUND: StatusCode = StatusCode(404);
    pub const METHOD_NOT_ALLOWED: StatusCode = StatusCode(405);
    pub const CONFLICT: StatusCode = StatusCode(409);
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

/// Error type returned by the HTTP server implementation.
#[derive(Debug, Error)]
pub enum HttpError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("malformed request line")]
    MalformedRequestLine,
    #[error("unsupported http method")]
    UnsupportedMethod,
    #[error("malformed header")]
    MalformedHeader,
    #[error("missing host header")]
    MissingHost,
    #[error("unsupported http version")]
    UnsupportedVersion,
    #[error("payload too large")]
    BodyTooLarge,
    #[error("timeout")]
    Timeout,
    #[error("handler error: {0}")]
    Handler(String),
    #[error("codec error: {0}")]
    Codec(#[from] codec::Error),
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

type Handler<State> =
    Arc<dyn Fn(Request<State>) -> BoxFuture<'static, Result<Response, HttpError>> + Send + Sync>;

struct Route<State> {
    method: Method,
    pattern: RoutePattern,
    handler: Handler<State>,
}

struct RoutePattern {
    segments: Vec<Segment>,
}

enum Segment {
    Literal(String),
    Param(String),
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
        }
    }
}

impl RoutePattern {
    fn parse(pattern: &str) -> Self {
        let segments = pattern
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|segment| {
                if let Some(name) = segment.strip_prefix(':') {
                    Segment::Param(name.to_string())
                } else {
                    Segment::Literal(segment.to_string())
                }
            })
            .collect();
        Self { segments }
    }

    fn matches(&self, path: &str, params: &mut HashMap<String, String>) -> bool {
        let mut param_buf = HashMap::new();
        let segs: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if segs.len() != self.segments.len() {
            return false;
        }
        for (expected, actual) in self.segments.iter().zip(segs.iter()) {
            match expected {
                Segment::Literal(lit) => {
                    if lit != actual {
                        return false;
                    }
                }
                Segment::Param(name) => {
                    param_buf.insert(name.clone(), (*actual).to_string());
                }
            }
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
            Some((path, qs)) => (
                path.to_string(),
                form_urlencoded::parse(qs.as_bytes()).into_owned().collect(),
            ),
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
        codec::deserialize(JSON_CODEC, &self.body).map_err(HttpError::from)
    }

    pub fn take_body(mut self) -> Vec<u8> {
        std::mem::take(&mut self.body)
    }

    fn set_params(&mut self, params: HashMap<String, String>) {
        self.params = params;
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
        self.body = codec::serialize(JSON_CODEC, value)?;
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
    state: Arc<State>,
}

impl<State> Router<State>
where
    State: Send + Sync + 'static,
{
    pub fn new(state: State) -> Self {
        Self {
            routes: Arc::new(Vec::new()),
            state: Arc::new(state),
        }
    }

    fn with_routes(state: Arc<State>, routes: Vec<Route<State>>) -> Self {
        Self {
            routes: Arc::new(routes),
            state,
        }
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
        Router::with_routes(self.state.clone(), routes)
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
}

impl<State> Clone for Router<State> {
    fn clone(&self) -> Self {
        Self {
            routes: self.routes.clone(),
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
                tracing::debug!(?err, "http connection error");
            }
        });
    }
}

async fn handle_connection<State>(
    stream: TcpStream,
    remote: SocketAddr,
    router: Router<State>,
    config: ServerConfig,
) -> Result<(), HttpError>
where
    State: Send + Sync + 'static,
{
    let mut stream = BufferedTcpStream::new(stream);
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
        let keep_alive = request.keep_alive();
        let keep_alive_allowed = keep_alive && !config.keep_alive.is_zero();
        let mut response = router.dispatch(request).await?;
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
    if response.keep_alive {
        headers.insert("connection".into(), "keep-alive".into());
    } else {
        headers.insert("connection".into(), "close".into());
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

async fn read_request<State>(
    stream: &mut BufferedTcpStream,
    remote: SocketAddr,
    state: Arc<State>,
    max_body: usize,
) -> Result<Option<Request<State>>, HttpError>
where
    State: Send + Sync + 'static,
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
