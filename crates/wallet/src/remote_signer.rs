use crate::{WalletError, WalletSigner};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hex;
use ledger::crypto::remote_tag;
use metrics::{histogram, increment_counter};
use native_tls::{Certificate as NativeCertificate, Identity, TlsConnector};
use once_cell::sync::Lazy;
use rand::Rng;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::net::UdpSocket;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use tungstenite::{client::IntoClientRequest, Message};
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
    client: Client,
    tls: Option<TlsConnector>,
    pubkeys: Vec<VerifyingKey>,
    timeout: Duration,
    retries: u8,
    threshold: usize,
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

    fn fetch_pubkey(client: &Client, endpoint: &str) -> Result<VerifyingKey, WalletError> {
        {
            let cache = PUBKEY_CACHE.lock().unwrap();
            if let Some((pk, ts)) = cache.get(endpoint) {
                if ts.elapsed() < PUBKEY_TTL {
                    return Ok(pk.clone());
                }
            }
        }
        let url = if endpoint.starts_with("ws") {
            endpoint
                .replacen("wss://", "https://", 1)
                .replacen("ws://", "http://", 1)
        } else {
            endpoint.to_string()
        };
        let resp = client
            .get(format!("{url}/pubkey"))
            .send()
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let pk: PubKeyResp = resp
            .json()
            .map_err(|e| WalletError::Failure(e.to_string()))?;
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
            .insert(endpoint.to_string(), (pubkey, Instant::now()));
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
        let mut builder = Client::builder().timeout(timeout);
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
                builder = builder
                    .add_root_certificate(
                        reqwest::Certificate::from_pem(&ca_bytes)
                            .map_err(|e| WalletError::Failure(e.to_string()))?,
                    )
                    .danger_accept_invalid_certs(true);
            }
            let connector = tls_builder
                .build()
                .map_err(|e| WalletError::Failure(e.to_string()))?;
            tls = Some(connector);
        }
        let client = builder
            .build()
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let mut pubkeys = Vec::new();
        for ep in endpoints {
            pubkeys.push(Self::fetch_pubkey(&client, ep)?);
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
                .post(format!("{endpoint}/sign"))
                .json(payload)
                .timeout(self.timeout)
                .send();
            match res {
                Ok(resp) => match resp.json::<SignResp>() {
                    Ok(r) => {
                        let sig_bytes =
                            hex::decode(r.sig).map_err(|e| WalletError::Failure(e.to_string()))?;
                        if sig_bytes.len() != 64 {
                            return Err(WalletError::Failure("invalid signature length".into()));
                        }
                        return Signature::from_slice(&sig_bytes)
                            .map_err(|e| WalletError::Failure(e.to_string()));
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
        let req = url
            .into_client_request()
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let host = req
            .uri()
            .host()
            .ok_or_else(|| WalletError::Failure("missing host".into()))?;
        let port = req.uri().port_u16().unwrap_or(443);
        let addr = format!("{host}:{port}");
        let tcp =
            std::net::TcpStream::connect(&addr).map_err(|e| WalletError::Failure(e.to_string()))?;
        let stream = if let Some(connector) = &self.tls {
            let tls_stream = connector
                .connect(host, tcp)
                .map_err(|e| WalletError::Failure(e.to_string()))?;
            tungstenite::stream::MaybeTlsStream::NativeTls(tls_stream)
        } else {
            tungstenite::stream::MaybeTlsStream::Plain(tcp)
        };
        let (mut socket, _) = tungstenite::client::client(req, stream)
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        socket
            .send(Message::Text(serde_json::to_string(payload).unwrap()))
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let msg = socket
            .read()
            .map_err(|e| WalletError::Failure(e.to_string()))?;
        let txt = match msg {
            Message::Text(t) => t,
            _ => return Err(WalletError::Failure("invalid ws response".into())),
        };
        let r: SignResp =
            serde_json::from_str(&txt).map_err(|e| WalletError::Failure(e.to_string()))?;
        let sig_bytes = hex::decode(r.sig).map_err(|e| WalletError::Failure(e.to_string()))?;
        if sig_bytes.len() != 64 {
            return Err(WalletError::Failure("invalid signature length".into()));
        }
        Signature::from_slice(&sig_bytes).map_err(|e| WalletError::Failure(e.to_string()))
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
