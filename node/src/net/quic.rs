use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use quinn::{Connection, Endpoint};
use rand::Rng;
use rcgen::generate_simple_self_signed;
#[cfg(any(test, debug_assertions))]
use rustls::client::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier, WebPkiVerifier,
};
use rustls::{Certificate, ClientConfig, PrivateKey, RootCertStore};
#[cfg(any(test, debug_assertions))]
use rustls::{DigitallySignedStruct, ServerName, SignatureScheme};
use tokio::time::Instant;

use super::peer::HandshakeError;
#[cfg(feature = "telemetry")]
use crate::telemetry::{
    sampled_observe, QUIC_BYTES_RECV_TOTAL, QUIC_BYTES_SENT_TOTAL, QUIC_CONN_LATENCY_SECONDS,
    QUIC_DISCONNECT_TOTAL, QUIC_ENDPOINT_REUSE_TOTAL, QUIC_HANDSHAKE_FAIL_TOTAL,
};

/// Error type for QUIC connection attempts.
#[derive(Debug)]
pub enum ConnectError {
    /// Handshake failed with the remote peer and includes a reason.
    Handshake(HandshakeError),
    /// Other connection failure.
    Other(anyhow::Error),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Handshake(e) => write!(f, "handshake failed: {}", e.as_str()),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// Start a QUIC listener bound to `addr`, returning the endpoint
/// and the generated self-signed certificate to share with peers.
pub async fn listen(addr: SocketAddr) -> Result<(Endpoint, Certificate)> {
    let cert = generate_simple_self_signed(["the-block".to_string()])?;
    let cert_der = cert.serialize_der()?;
    let key_der = cert.serialize_private_key_der();
    let cert = Certificate(cert_der.clone());
    let key = PrivateKey(key_der);
    let server_config = quinn::ServerConfig::with_single_cert(vec![cert.clone()], key)?;
    let mut attempts = 0;
    loop {
        match Endpoint::server(server_config.clone(), addr) {
            Ok(endpoint) => return Ok((endpoint, cert)),
            Err(_e) if attempts < 3 => {
                attempts += 1;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
            Err(e) => return Err(anyhow!(e)),
        }
    }
}

/// Start a QUIC listener with an existing certificate and key.
pub async fn listen_with_cert(
    addr: SocketAddr,
    cert_der: &[u8],
    key_der: &[u8],
) -> Result<Endpoint> {
    let cert = Certificate(cert_der.to_vec());
    let key = PrivateKey(key_der.to_vec());
    let server_config = quinn::ServerConfig::with_single_cert(vec![cert], key)?;
    let mut attempts = 0;
    loop {
        match Endpoint::server(server_config.clone(), addr) {
            Ok(endpoint) => return Ok(endpoint),
            Err(_e) if attempts < 3 => {
                attempts += 1;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
            Err(e) => return Err(anyhow!(e)),
        }
    }
}

/// Connect to a remote QUIC endpoint at `addr` trusting `cert`.
pub async fn connect(
    addr: SocketAddr,
    cert: Certificate,
) -> std::result::Result<Connection, ConnectError> {
    let mut roots = RootCertStore::empty();
    roots
        .add(&cert)
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let crypto = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let client_cfg = quinn::ClientConfig::new(Arc::new(crypto));
    let endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let _start = Instant::now();
    let attempt = endpoint
        .connect_with(client_cfg, addr, "the-block")
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let res = tokio::time::timeout(std::time::Duration::from_secs(5), attempt).await;
    match res {
        Ok(Ok(conn)) => {
            let elapsed = _start.elapsed();
            #[cfg(feature = "telemetry")]
            sampled_observe(&QUIC_CONN_LATENCY_SECONDS, elapsed.as_secs_f64());
            if let Some(pk) = super::peer::pk_from_addr(&addr) {
                super::peer::record_handshake_latency(&pk, elapsed.as_millis() as u64);
            }
            Ok(conn)
        }
        Ok(Err(e)) => {
            let err = classify_err(&e);
            #[cfg(feature = "telemetry")]
            {
                if super::peer::track_handshake_fail_enabled() {
                    QUIC_HANDSHAKE_FAIL_TOTAL
                        .with_label_values(&[err.as_str()])
                        .inc();
                }
                super::peer::record_handshake_fail_addr(addr, err);
            }
            tracing::error!(error = ?e, reason = err.as_str(), "quic_connect_fail");
            Err(ConnectError::Handshake(err))
        }
        Err(_) => {
            let err = super::peer::HandshakeError::Timeout;
            #[cfg(feature = "telemetry")]
            {
                if super::peer::track_handshake_fail_enabled() {
                    QUIC_HANDSHAKE_FAIL_TOTAL
                        .with_label_values(&[err.as_str()])
                        .inc();
                }
                super::peer::record_handshake_fail_addr(addr, err);
            }
            tracing::error!("quic_connect_timeout");
            Err(ConnectError::Handshake(err))
        }
    }
}

static CONNECTIONS: Lazy<DashMap<SocketAddr, Connection>> = Lazy::new(|| DashMap::new());

/// Obtain a cached QUIC connection to `addr` if available, otherwise establish
/// a new one. Reused connections are counted via `QUIC_ENDPOINT_REUSE_TOTAL`.
pub async fn get_connection(
    addr: SocketAddr,
    cert: Certificate,
) -> std::result::Result<Connection, ConnectError> {
    if let Some(existing) = CONNECTIONS.get(&addr) {
        if existing.close_reason().is_none() {
            #[cfg(feature = "telemetry")]
            QUIC_ENDPOINT_REUSE_TOTAL.inc();
            return Ok(existing.clone());
        } else {
            CONNECTIONS.remove(&addr);
        }
    }
    let conn = connect(addr, cert).await?;
    CONNECTIONS.insert(addr, conn.clone());
    Ok(conn)
}

/// Drop a pooled connection for `addr` if present.
pub fn drop_connection(addr: &SocketAddr) {
    CONNECTIONS.remove(addr);
}

/// Connect to `addr` without verifying the remote certificate.
///
/// Only available in tests or debug builds. Production code should use
/// [`connect`] to enforce certificate validation.
#[cfg(any(test, debug_assertions))]
pub async fn connect_insecure(addr: SocketAddr) -> std::result::Result<Connection, ConnectError> {
    struct SkipCertVerification;
    impl ServerCertVerifier for SkipCertVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &Certificate,
            _intermediates: &[Certificate],
            _server_name: &ServerName,
            _scts: &mut dyn Iterator<Item = &[u8]>,
            _ocsp_response: &[u8],
            _now: std::time::SystemTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &Certificate,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &Certificate,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            WebPkiVerifier::new(RootCertStore::empty(), None).supported_verify_schemes()
        }
    }
    let verifier = Arc::new(SkipCertVerification);
    let crypto = ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let client_cfg = quinn::ClientConfig::new(Arc::new(crypto));
    let endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let _start = Instant::now();
    let attempt = endpoint
        .connect_with(client_cfg, addr, "the-block")
        .map_err(|e| ConnectError::Other(anyhow!(e)))?;
    let res = tokio::time::timeout(std::time::Duration::from_secs(5), attempt).await;
    match res {
        Ok(Ok(conn)) => {
            #[cfg(feature = "telemetry")]
            sampled_observe(&QUIC_CONN_LATENCY_SECONDS, _start.elapsed().as_secs_f64());
            Ok(conn)
        }
        Ok(Err(e)) => {
            let err = classify_err(&e);
            #[cfg(feature = "telemetry")]
            {
                if super::peer::track_handshake_fail_enabled() {
                    QUIC_HANDSHAKE_FAIL_TOTAL
                        .with_label_values(&[err.as_str()])
                        .inc();
                }
                super::peer::record_handshake_fail_addr(addr, err);
            }
            tracing::error!(error = ?e, reason = err.as_str(), "quic_connect_fail");
            Err(ConnectError::Handshake(err))
        }
        Err(_) => {
            let err = super::peer::HandshakeError::Timeout;
            #[cfg(feature = "telemetry")]
            {
                if super::peer::track_handshake_fail_enabled() {
                    QUIC_HANDSHAKE_FAIL_TOTAL
                        .with_label_values(&[err.as_str()])
                        .inc();
                }
                super::peer::record_handshake_fail_addr(addr, err);
            }
            tracing::error!("quic_connect_timeout");
            Err(ConnectError::Handshake(err))
        }
    }
}

#[cfg(feature = "telemetry")]
pub(crate) fn classify_err(e: &quinn::ConnectionError) -> HandshakeError {
    let msg = e.to_string().to_lowercase();
    if msg.contains("certificate") {
        HandshakeError::Certificate
    } else if msg.contains("tls") {
        HandshakeError::Tls
    } else if msg.contains("timeout") {
        HandshakeError::Timeout
    } else if msg.contains("version") {
        HandshakeError::Version
    } else {
        HandshakeError::Other
    }
}

/// Send raw bytes over a QUIC uni-stream recording telemetry counters.
pub async fn send(conn: &Connection, data: &[u8]) -> Result<()> {
    let mut rng = rand::thread_rng();
    if let Ok(loss_str) = std::env::var("TB_QUIC_PACKET_LOSS") {
        if let Ok(loss) = loss_str.parse::<f64>() {
            if rng.gen_bool(loss) {
                return Ok(());
            }
        }
    }
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
    if let Ok(dup_str) = std::env::var("TB_QUIC_PACKET_DUP") {
        if let Ok(dup) = dup_str.parse::<f64>() {
            if rng.gen_bool(dup) {
                let _ = stream.write_all(data).await;
            }
        }
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

/// Receive a single uni-stream from `conn`, returning the bytes if any.
pub async fn recv(conn: &Connection) -> Option<Vec<u8>> {
    match conn.accept_uni().await {
        Ok(mut s) => match s.read_to_end(usize::MAX).await {
            Ok(buf) => {
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
        },
        Err(e) => {
            #[cfg(feature = "telemetry")]
            record_conn_err(&e);
            #[cfg(not(feature = "telemetry"))]
            let _ = e;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_err_variants() {
        assert_eq!(
            classify_err(&quinn::ConnectionError::VersionMismatch),
            HandshakeError::Version
        );
        assert_eq!(
            classify_err(&quinn::ConnectionError::TimedOut),
            HandshakeError::Timeout
        );
    }
}

#[cfg(feature = "telemetry")]
fn record_conn_err(e: &quinn::ConnectionError) {
    let code: u64 = match e {
        quinn::ConnectionError::ApplicationClosed(ac) => ac.error_code.into(),
        quinn::ConnectionError::ConnectionClosed(cc) => cc.error_code.into(),
        quinn::ConnectionError::Reset => 0,
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
