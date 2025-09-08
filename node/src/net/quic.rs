use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use futures::StreamExt;
use once_cell::sync::Lazy;
use quinn::{rustls, Connection, Endpoint, Incoming};
use rcgen::generate_simple_self_signed;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;

#[cfg(feature = "telemetry")]
use crate::telemetry::{
    QUIC_BYTES_RECV_TOTAL, QUIC_BYTES_SENT_TOTAL, QUIC_CONN_LATENCY_SECONDS, QUIC_DISCONNECT_TOTAL,
    QUIC_ENDPOINT_REUSE_TOTAL, QUIC_HANDSHAKE_FAIL_TOTAL,
};

/// Error type for QUIC connection attempts.
#[derive(Debug)]
pub enum ConnectError {
    /// Handshake failed with the remote peer.
    Handshake,
    /// Other connection failure.
    Other(anyhow::Error),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Handshake => write!(f, "handshake failed"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConnectError {}

static ENDPOINT_POOL: Lazy<Mutex<HashMap<SocketAddr, Endpoint>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn get_or_create_endpoint(addr: SocketAddr) -> Result<Endpoint> {
    let mut pool = ENDPOINT_POOL.lock().unwrap();
    if let Some(ep) = pool.get(&addr) {
        #[cfg(feature = "telemetry")]
        QUIC_ENDPOINT_REUSE_TOTAL.inc();
        return Ok(ep.clone());
    }
    let ep = Endpoint::client(addr)?;
    pool.insert(addr, ep.clone());
    Ok(ep)
}

/// Start a QUIC listener bound to `addr`, returning the endpoint, incoming stream
/// handle, and the generated self-signed certificate to share with peers.
pub async fn listen(addr: SocketAddr) -> Result<(Endpoint, Incoming, quinn::Certificate)> {
    let cert = generate_simple_self_signed(["the-block".to_string()])?;
    let cert_der = cert.serialize_der()?;
    let key_der = cert.serialize_private_key_der();
    let cert = quinn::Certificate::from_der(&cert_der)?;
    let key = quinn::PrivateKey::from_der(&key_der)?;
    let server_config = quinn::ServerConfig::with_single_cert(vec![cert.clone()], key)?;
    let (endpoint, incoming) = Endpoint::server(server_config, addr)?;
    Ok((endpoint, incoming, cert))
}

/// Start a QUIC listener with an existing certificate and key.
pub async fn listen_with_cert(
    addr: SocketAddr,
    cert_der: &[u8],
    key_der: &[u8],
) -> Result<(Endpoint, Incoming)> {
    let cert = quinn::Certificate::from_der(cert_der)?;
    let key = quinn::PrivateKey::from_der(key_der)?;
    let server_config = quinn::ServerConfig::with_single_cert(vec![cert], key)?;
    let (endpoint, incoming) = Endpoint::server(server_config, addr)?;
    Ok((endpoint, incoming))
}

/// Connect to a remote QUIC endpoint at `addr` trusting `cert`.
pub async fn connect(
    addr: SocketAddr,
    cert: quinn::Certificate,
) -> std::result::Result<Connection, ConnectError> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls::Certificate(cert.clone().into_der()))
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let client_cfg = quinn::ClientConfig::new(Arc::new(crypto));
    let endpoint =
        get_or_create_endpoint("0.0.0.0:0".parse().unwrap()).map_err(ConnectError::Other)?;
    let start = Instant::now();
    let attempt = endpoint
        .connect_with(client_cfg, addr, "the-block")
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    match attempt.await {
        Ok(conn) => {
            #[cfg(feature = "telemetry")]
            QUIC_CONN_LATENCY_SECONDS.observe(start.elapsed().as_secs_f64());
            Ok(conn.connection)
        }
        Err(e) => {
            #[cfg(feature = "telemetry")]
            {
                QUIC_HANDSHAKE_FAIL_TOTAL.inc();
                let reason = if e.to_string().to_lowercase().contains("certificate") {
                    "bad_cert"
                } else if e.to_string().to_lowercase().contains("timeout") {
                    "timeout"
                } else {
                    "other"
                };
                super::peer::record_handshake_fail_addr(addr, reason);
            }
            Err(ConnectError::Handshake)
        }
    }
}

/// Connect to `addr` without verifying the remote certificate.
///
/// Only available in tests or debug builds. Production code should use
/// [`connect`] to enforce certificate validation.
#[cfg(any(test, debug_assertions))]
pub async fn connect_insecure(addr: SocketAddr) -> std::result::Result<Connection, ConnectError> {
    struct SkipCertVerification;
    impl rustls::client::ServerCertVerifier for SkipCertVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &rustls::Certificate,
            _intermediates: &[rustls::Certificate],
            _server_name: &rustls::ServerName,
            _scts: &mut dyn Iterator<Item = &[u8]>,
            _ocsp_response: &[u8],
            _now: std::time::SystemTime,
        ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::Certificate,
            _dss: &rustls::client::Tls12Signer,
        ) -> Result<rustls::client::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::Certificate,
            _dss: &rustls::client::Tls13Signer,
        ) -> Result<rustls::client::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::client::WebPkiVerifier::supported_verify_schemes()
        }
    }
    let verifier = Arc::new(SkipCertVerification);
    let crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let client_cfg = quinn::ClientConfig::new(Arc::new(crypto));
    let endpoint =
        get_or_create_endpoint("0.0.0.0:0".parse().unwrap()).map_err(ConnectError::Other)?;
    let start = Instant::now();
    let attempt = endpoint.connect_with(client_cfg, addr, "the-block")?;
    match attempt.await {
        Ok(conn) => {
            #[cfg(feature = "telemetry")]
            QUIC_CONN_LATENCY_SECONDS.observe(start.elapsed().as_secs_f64());
            Ok(conn.connection)
        }
        Err(e) => {
            #[cfg(feature = "telemetry")]
            {
                QUIC_HANDSHAKE_FAIL_TOTAL.inc();
                let reason = if e.to_string().to_lowercase().contains("certificate") {
                    "bad_cert"
                } else if e.to_string().to_lowercase().contains("timeout") {
                    "timeout"
                } else {
                    "other"
                };
                super::peer::record_handshake_fail_addr(addr, reason);
            }
            Err(ConnectError::Handshake)
        }
    }
}

/// Send raw bytes over a QUIC uni-stream recording telemetry counters.
pub async fn send(conn: &Connection, data: &[u8]) -> Result<()> {
    let mut stream = match conn.open_uni().await {
        Ok(s) => s,
        Err(e) => {
            #[cfg(feature = "telemetry")]
            record_conn_err(&e);
            return Err(e.into());
        }
    };
    if let Err(e) = stream.write_all(data).await {
        #[cfg(feature = "telemetry")]
        record_write_err(&e);
        return Err(e.into());
    }
    #[cfg(feature = "telemetry")]
    QUIC_BYTES_SENT_TOTAL.inc_by(data.len() as u64);
    if let Err(e) = stream.finish().await {
        #[cfg(feature = "telemetry")]
        record_write_err(&e);
        return Err(e.into());
    }
    Ok(())
}

/// Receive a single uni-stream from `incoming`, returning the bytes if any.
pub async fn recv(incoming: &mut quinn::IncomingUniStreams) -> Option<Vec<u8>> {
    if let Some(stream) = incoming.next().await {
        match stream {
            Ok(mut s) => {
                let mut buf = Vec::new();
                match s.read_to_end(&mut buf).await {
                    Ok(_) => {
                        #[cfg(feature = "telemetry")]
                        QUIC_BYTES_RECV_TOTAL.inc_by(buf.len() as u64);
                        Some(buf)
                    }
                    Err(_) => {
                        #[cfg(feature = "telemetry")]
                        QUIC_DISCONNECT_TOTAL
                            .with_label_values(&["read_error"])
                            .inc();
                        None
                    }
                }
            }
            Err(e) => {
                #[cfg(feature = "telemetry")]
                record_conn_err(&e);
                None
            }
        }
    } else {
        None
    }
}

#[cfg(feature = "telemetry")]
fn record_conn_err(e: &quinn::ConnectionError) {
    use std::convert::From;
    let code: u64 = match e {
        quinn::ConnectionError::ApplicationClosed(ac) => ac.error_code.into(),
        quinn::ConnectionError::ConnectionClosed(cc) => cc.error_code.into(),
        quinn::ConnectionError::Reset(r) => r.error_code.into(),
        quinn::ConnectionError::TransportError(te) => te.code.into(),
        _ => 0,
    };
    let label = code.to_string();
    QUIC_DISCONNECT_TOTAL.with_label_values(&[&label]).inc();
}

#[cfg(feature = "telemetry")]
fn record_write_err(e: &quinn::WriteError) {
    match e {
        quinn::WriteError::ConnectionLost(conn) => record_conn_err(conn),
        quinn::WriteError::Stopped(code) => {
            let label = u64::from(*code).to_string();
            QUIC_DISCONNECT_TOTAL.with_label_values(&[&label]).inc();
        }
        _ => {
            QUIC_DISCONNECT_TOTAL.with_label_values(&["0"]).inc();
        }
    }
}
