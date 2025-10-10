use std::sync::Arc;
use std::time::Duration;

use foundation_serialization::json;
use foundation_serialization::{Deserialize, Serialize};
use httpd::{
    HttpError, Method, Response, Router, ServerConfig, ServerTlsConfig, StatusCode,
    WebSocketResponse,
};
use runtime::{self, net::TcpListener, ws::Message};
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
    handle: runtime::JoinHandle<std::io::Result<()>>,
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
        runtime::block_on(async move {
            let wallet = Wallet::generate();
            let pk_hex = wallet.public_key_hex();
            let state = SignerState {
                wallet: Arc::new(wallet),
                pk_hex,
                behavior,
            };
            let router = build_http_router(state.clone());
            let (url, handle) = spawn_httpd(router, "http", None).await;
            HttpSignerMock { url, handle }
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for HttpSignerMock {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[allow(dead_code)]
pub struct TlsWebSocketSignerMock {
    url: String,
    handle: runtime::JoinHandle<std::io::Result<()>>,
}

#[allow(dead_code)]
impl TlsWebSocketSignerMock {
    pub fn new(wallet: Wallet, tls: ServerTlsConfig) -> Self {
        runtime::block_on(async move {
            let pk_hex = wallet.public_key_hex();
            let state = SignerState {
                wallet: Arc::new(wallet),
                pk_hex,
                behavior: SignerBehavior::Success,
            };
            let router = build_websocket_router(state.clone());
            let (url, handle) = spawn_httpd(router, "wss", Some(tls)).await;
            TlsWebSocketSignerMock { url, handle }
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for TlsWebSocketSignerMock {
    fn drop(&mut self) {
        self.handle.abort();
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

#[allow(dead_code)]
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
                            ))
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

async fn spawn_httpd(
    router: Router<SignerState>,
    scheme: &str,
    tls: Option<ServerTlsConfig>,
) -> (String, runtime::JoinHandle<std::io::Result<()>>) {
    let listener = TcpListener::bind("127.0.0.1:0".parse().expect("bind addr"))
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener address");
    let url = format!("{scheme}://{addr}");
    let handle = if let Some(tls_cfg) = tls {
        runtime::spawn(async move {
            httpd::serve_tls(listener, router, ServerConfig::default(), tls_cfg).await
        })
    } else {
        runtime::spawn(async move { httpd::serve(listener, router, ServerConfig::default()).await })
    };
    (url, handle)
}
