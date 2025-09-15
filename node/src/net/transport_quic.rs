#![cfg(feature = "quic")]
use std::net::SocketAddr;

use s2n_quic::{client::Connect, Client, Server};

#[cfg(feature = "telemetry")]
use crate::telemetry::{QUIC_HANDSHAKE_FAIL_TOTAL, QUIC_RETRANSMIT_TOTAL};

/// Start a QUIC server bound to `addr`.
pub async fn start_server(addr: SocketAddr) -> Result<Server, Box<dyn std::error::Error>> {
    let server = Server::builder()
        .with_default_tls_config()?
        .with_io(addr)?
        .start()
        .await?;
    Ok(server)
}

/// Establish a QUIC connection to `addr` using default TLS.
pub async fn connect(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .with_default_tls_config()?
        .with_io("0.0.0.0:0".parse::<SocketAddr>()?)?
        .start()
        .await?;
    let _ = client
        .connect(Connect::new(addr, "the-block"))
        .await?
        .into_stream();
    Ok(())
}

/// Record a handshake failure for telemetry.
pub fn record_handshake_fail(reason: &str) {
    #[cfg(feature = "telemetry")]
    QUIC_HANDSHAKE_FAIL_TOTAL.with_label_values(&[reason]).inc();
}

/// Record a retransmission event for telemetry.
pub fn record_retransmit(count: u64) {
    #[cfg(feature = "telemetry")]
    QUIC_RETRANSMIT_TOTAL.inc_by(count);
}
