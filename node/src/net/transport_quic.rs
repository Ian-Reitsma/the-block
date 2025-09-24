#![cfg(feature = "quic")]

use std::net::SocketAddr;

use anyhow::{anyhow, Result};

use ed25519_dalek::SigningKey;

use transport::{self, CertAdvertisement, ListenerHandle, LocalCert};

fn s2n_adapter() -> Result<transport::S2nAdapter> {
    super::transport_registry()
        .and_then(|registry| registry.s2n())
        .ok_or_else(|| anyhow!("s2n transport provider not configured"))
}

pub fn initialize(signing_key: &SigningKey) -> Result<CertAdvertisement> {
    let adapter = s2n_adapter()?;
    adapter.initialize(signing_key)
}

pub fn rotate(signing_key: &SigningKey) -> Result<CertAdvertisement> {
    let adapter = s2n_adapter()?;
    adapter.rotate(signing_key)
}

pub fn current_cert() -> Option<LocalCert> {
    s2n_adapter()
        .ok()
        .and_then(|adapter| adapter.current_cert())
}

pub fn current_advertisement() -> Option<CertAdvertisement> {
    s2n_adapter()
        .ok()
        .and_then(|adapter| adapter.current_advertisement())
}

pub fn fingerprint_history() -> Vec<[u8; 32]> {
    s2n_adapter()
        .map(|adapter| adapter.fingerprint_history())
        .unwrap_or_else(|_| transport::fingerprint_history())
}

pub fn fingerprint(cert: &[u8]) -> [u8; 32] {
    s2n_adapter()
        .map(|adapter| adapter.fingerprint(cert))
        .unwrap_or_else(|_| transport::fingerprint(cert))
}

pub fn verify_remote_certificate(peer_key: &[u8; 32], cert: &[u8]) -> Result<[u8; 32]> {
    match s2n_adapter() {
        Ok(adapter) => adapter.verify_remote_certificate(peer_key, cert),
        Err(_) => transport::verify_remote_certificate(peer_key, cert),
    }
}

pub async fn start_server(addr: SocketAddr) -> Result<ListenerHandle, Box<dyn std::error::Error>> {
    let adapter = s2n_adapter().map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
    let key = super::load_net_key();
    adapter.start_server(addr, &key).await
}

pub async fn connect(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let adapter = s2n_adapter().map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
    adapter.connect(addr).await
}

pub fn record_handshake_fail(reason: &str) {
    if let Ok(adapter) = s2n_adapter() {
        adapter.record_handshake_fail(reason);
    }
}

pub fn record_retransmit(count: u64) {
    if let Ok(adapter) = s2n_adapter() {
        adapter.record_retransmit(count);
    }
}
