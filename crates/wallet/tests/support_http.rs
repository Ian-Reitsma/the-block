use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use foundation_serialization::json;
use foundation_serialization::{Deserialize, Serialize};
use httpd::{
    HttpError, Method, Response, Router, ServerConfig, ServerTlsConfig, StatusCode,
    WebSocketResponse,
};
use runtime::{self, ws::Message};
use wallet::{Wallet, WalletError, WalletSigner};

#[derive(Clone)]
pub enum SignerBehavior {
    Success,
    Failure(StatusCode),
    InvalidSignature,
    Delay(Duration),
}

#[derive(Clone)]
struct SignerState {
    wallet: Arc<Wallet>,
    pk_hex: String,
    behavior: SignerBehavior,
}

#[derive(Serialize)]
struct PubKeyResponse {
    pubkey: String,
}

#[derive(Deserialize)]
struct SignRequest {
    msg: String,
}

#[derive(Serialize)]
struct SignResponse {
    sig: String,
}

fn success_response(state: &SignerState, request: SignRequest) -> Result<Response, HttpError> {
    let msg = crypto_suite::hex::decode(&request.msg)
        .map_err(|err| HttpError::Handler(format!("invalid hex payload: {err}")))?;
    let sig = state
        .wallet
        .sign(&msg)
        .map_err(|err: WalletError| HttpError::Handler(err.to_string()))?;
    Response::new(StatusCode::OK).json(&SignResponse {
        sig: crypto_suite::hex::encode(sig.to_bytes()),
    })
}

pub struct HttpSignerMock {
    url: String,
    shutdown: Arc<AtomicBool>,
    _thread: thread::JoinHandle<()>,
}

impl HttpSignerMock {
    pub fn success() -> Self {
        Self::with_behavior(SignerBehavior::Success)
    }

    pub fn failing(status: StatusCode) -> Self {
        Self::with_behavior(SignerBehavior::Failure(status))
    }

    pub fn invalid_signature() -> Self {
        Self::with_behavior(SignerBehavior::InvalidSignature)
    }

    pub fn delayed(duration: Duration) -> Self {
        Self::with_behavior(SignerBehavior::Delay(duration))
    }

    pub fn with_behavior(behavior: SignerBehavior) -> Self {
        let wallet = Wallet::generate();
        let pk_hex = wallet.public_key_hex();
        let state = SignerState {
            wallet: Arc::new(wallet),
            pk_hex,
            behavior,
        };
        let (url, shutdown, thread) = spawn_threaded_httpd(state, "http", None);
        HttpSignerMock {
            url,
            shutdown,
            _thread: thread,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for HttpSignerMock {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Thread will exit on its own when shutdown is set
    }
}

fn build_http_router(state: SignerState) -> Router<SignerState> {
    Router::new(state.clone())
        .route(Method::Get, "/pubkey", |req| async move {
            let state = req.state().clone();
            Response::new(StatusCode::OK).json(&PubKeyResponse {
                pubkey: state.pk_hex.clone(),
            })
        })
        .route(Method::Post, "/sign", |req| async move {
            let state = req.state().clone();
            match state.behavior.clone() {
                SignerBehavior::Failure(status) => Ok(Response::new(status)),
                SignerBehavior::InvalidSignature => {
                    let _ = req.json::<SignRequest>()?;
                    Response::new(StatusCode::OK).json(&SignResponse {
                        sig: "00".to_string(),
                    })
                }
                SignerBehavior::Delay(duration) => {
                    runtime::sleep(duration).await;
                    let payload = req.json::<SignRequest>()?;
                    success_response(&state, payload)
                }
                SignerBehavior::Success => {
                    let payload = req.json::<SignRequest>()?;
                    success_response(&state, payload)
                }
            }
        })
}

fn build_websocket_router(state: SignerState) -> Router<SignerState> {
    build_http_router(state.clone()).upgrade("/sign", |req, _| async move {
        let state = req.state().clone();
        Ok(WebSocketResponse::accept(move |mut stream| {
            let state = state.clone();
            Box::pin(async move {
                while let Some(message) = stream.recv().await.map_err(HttpError::from)? {
                    match message {
                        Message::Text(body) => {
                            let payload: SignRequest = json::from_str(&body)
                                .map_err(|err| HttpError::Handler(err.to_string()))?;
                            let response = success_response(&state, payload)?;
                            let text = String::from_utf8(response.body().to_vec())
                                .map_err(|err| HttpError::Handler(err.to_string()))?;
                            stream
                                .send(Message::Text(text))
                                .await
                                .map_err(HttpError::from)?;
                            break;
                        }
                        Message::Binary(_) => {
                            return Err(HttpError::Handler(
                                "binary websocket frames are not supported".into(),
                            ));
                        }
                        Message::Ping(data) => {
                            stream
                                .send(Message::Pong(data))
                                .await
                                .map_err(HttpError::from)?;
                        }
                        Message::Pong(_) => {}
                        Message::Close(_) => break,
                    }
                }
                stream.close().await.map_err(HttpError::from)?;
                Ok(())
            })
        }))
    })
}

/// Spawns a simple HTTP server on a separate OS thread.
/// Uses blocking I/O to avoid runtime complexities.
fn spawn_threaded_httpd(
    state: SignerState,
    scheme: &str,
    tls: Option<ServerTlsConfig>,
) -> (String, Arc<AtomicBool>, thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener as StdTcpListener;
    use std::sync::mpsc;

    let (addr_tx, addr_rx) = mpsc::channel();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    // For TLS, we still need to use the runtime approach
    if tls.is_some() {
        // Fall back to original implementation for TLS
        let thread = thread::spawn(move || {
            runtime::block_on(async move {
                let listener = runtime::net::TcpListener::bind("127.0.0.1:0".parse().unwrap())
                    .await
                    .expect("bind listener");
                let addr = listener.local_addr().expect("listener address");
                addr_tx.send(addr).expect("send addr");

                let tls_cfg = tls.unwrap();
                let router = build_websocket_router(state);
                let mut config = ServerConfig::default();
                config.keep_alive = Duration::ZERO;

                loop {
                    if shutdown_clone.load(Ordering::SeqCst) {
                        break;
                    }
                    let accept_result =
                        runtime::timeout(Duration::from_millis(100), listener.accept()).await;
                    match accept_result {
                        Ok(Ok((stream, remote))) => {
                            if shutdown_clone.load(Ordering::SeqCst) {
                                break;
                            }
                            let _ = httpd::serve_tls_stream(
                                stream,
                                remote,
                                router.clone(),
                                config.clone(),
                                tls_cfg.clone(),
                            )
                            .await;
                        }
                        _ => continue,
                    }
                }
            });
        });

        let addr = addr_rx.recv().expect("recv addr");
        let url = format!("{scheme}://127.0.0.1:{}", addr.port());
        thread::sleep(Duration::from_millis(50));
        return (url, shutdown, thread);
    }

    // For plain HTTP, use simple blocking I/O
    let thread = thread::spawn(move || {
        let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        addr_tx.send(addr).expect("send addr");

        for stream in listener.incoming() {
            if shutdown_clone.load(Ordering::SeqCst) {
                break;
            }

            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Set read timeout to avoid blocking forever
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

            // Read HTTP request
            let mut reader = BufReader::new(&stream);
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }

            // Parse method and path
            let parts: Vec<&str> = request_line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let method = parts[0];
            let path = parts[1];

            // Read headers
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line == "\r\n" || line == "\n" {
                    break;
                }
                if line.to_lowercase().starts_with("content-length:") {
                    if let Some(len) = line.split(':').nth(1) {
                        content_length = len.trim().parse().unwrap_or(0);
                    }
                }
            }

            // Read body if present
            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                if reader.read_exact(&mut body).is_err() {
                    continue;
                }
            }

            // Handle request
            let response = match (method, path) {
                ("GET", "/pubkey") => {
                    let json = format!(r#"{{"pubkey":"{}"}}"#, state.pk_hex);
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        json.len(),
                        json
                    )
                }
                ("POST", "/sign") => match handle_sign_request(&state, &body) {
                    Ok(json) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        json.len(),
                        json
                    ),
                    Err(status) => format!("HTTP/1.1 {}\r\nConnection: close\r\n\r\n", status),
                },
                _ => "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n".to_string(),
            };

            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    let addr = addr_rx.recv().expect("recv addr");
    let url = format!("{scheme}://127.0.0.1:{}", addr.port());
    thread::sleep(Duration::from_millis(50));
    (url, shutdown, thread)
}

fn handle_sign_request(state: &SignerState, body: &[u8]) -> Result<String, u16> {
    let body_str = std::str::from_utf8(body).map_err(|_| 400u16)?;

    // Use a simple manual JSON parser to avoid serde issues with extra fields
    let msg_value = extract_json_field(body_str, "msg").ok_or(400u16)?;

    match &state.behavior {
        SignerBehavior::Success => {
            let msg = crypto_suite::hex::decode(&msg_value).map_err(|_| 400u16)?;
            let sig = state.wallet.sign(&msg).map_err(|_| 500u16)?;
            let sig_hex = crypto_suite::hex::encode(sig.to_bytes());
            Ok(format!(r#"{{"sig":"{}"}}"#, sig_hex))
        }
        SignerBehavior::Failure(status) => Err(status.as_u16()),
        SignerBehavior::InvalidSignature => Ok(r#"{"sig":"00"}"#.to_string()),
        SignerBehavior::Delay(duration) => {
            thread::sleep(*duration);
            let msg = crypto_suite::hex::decode(&msg_value).map_err(|_| 400u16)?;
            let sig = state.wallet.sign(&msg).map_err(|_| 500u16)?;
            let sig_hex = crypto_suite::hex::encode(sig.to_bytes());
            Ok(format!(r#"{{"sig":"{}"}}"#, sig_hex))
        }
    }
}

/// Simple JSON field extractor that doesn't require serde
fn extract_json_field(json: &str, field: &str) -> Option<String> {
    let pattern = format!(r#""{}":"#, field);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];

    // Find the value - handle string values
    if rest.starts_with('"') {
        let rest = &rest[1..]; // skip opening quote
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else {
        // Handle non-string values
        let end = rest.find(|c: char| c == ',' || c == '}')?;
        Some(rest[..end].to_string())
    }
}
