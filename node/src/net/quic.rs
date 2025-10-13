#![cfg(feature = "quinn")]

use diagnostics::anyhow::{anyhow, Result};
use std::net::SocketAddr;

use transport::{self, ConnectionHandle, ListenerHandle};

pub use transport::{
    classify_err, CertificateHandle, ConnectError, ConnectionStatsSnapshot, HandshakeError,
    QuinnDisconnect,
};

fn quinn_adapter() -> Result<transport::QuinnAdapter> {
    super::transport_registry()
        .and_then(|registry| registry.quinn())
        .ok_or_else(|| anyhow!("quinn transport provider not configured"))
}

pub async fn listen(addr: SocketAddr) -> Result<(ListenerHandle, CertificateHandle)> {
    let adapter = quinn_adapter()?;
    adapter.listen(addr).await.map_err(Into::into)
}

pub async fn listen_with_cert(
    addr: SocketAddr,
    cert_der: concurrency::Bytes,
    key_der: concurrency::Bytes,
) -> Result<ListenerHandle> {
    let adapter = quinn_adapter()?;
    adapter
        .listen_with_cert(addr, cert_der, key_der)
        .await
        .map_err(Into::into)
}

pub async fn listen_with_chain(
    addr: SocketAddr,
    chain: &[concurrency::Bytes],
    key_der: concurrency::Bytes,
) -> Result<ListenerHandle> {
    let adapter = quinn_adapter()?;
    adapter
        .listen_with_chain(addr, chain, key_der)
        .await
        .map_err(Into::into)
}

pub async fn connect(
    addr: SocketAddr,
    cert: &CertificateHandle,
) -> std::result::Result<ConnectionHandle, ConnectError> {
    let adapter = quinn_adapter().map_err(ConnectError::Other)?;
    adapter.connect(addr, cert).await
}

pub async fn get_connection(
    addr: SocketAddr,
    cert: &CertificateHandle,
) -> std::result::Result<ConnectionHandle, ConnectError> {
    let adapter = quinn_adapter().map_err(ConnectError::Other)?;
    adapter.get_connection(addr, cert).await
}

pub fn drop_connection(addr: &SocketAddr) {
    if let Ok(adapter) = quinn_adapter() {
        adapter.drop_connection(addr);
    }
}

pub fn connection_stats() -> Vec<(SocketAddr, ConnectionStatsSnapshot)> {
    quinn_adapter()
        .map(|adapter| adapter.connection_stats())
        .unwrap_or_default()
}

pub async fn send(conn: &ConnectionHandle, data: &[u8]) -> Result<()> {
    let adapter = quinn_adapter()?;
    adapter.send(conn, data).await
}

pub async fn recv(conn: &ConnectionHandle) -> Option<Vec<u8>> {
    match quinn_adapter() {
        Ok(adapter) => adapter.recv(conn).await,
        Err(_) => None,
    }
}

#[cfg(any(test, debug_assertions))]
pub async fn connect_insecure(
    addr: SocketAddr,
) -> std::result::Result<ConnectionHandle, ConnectError> {
    let adapter = quinn_adapter().map_err(ConnectError::Other)?;
    adapter.connect_insecure(addr).await
}

pub fn certificate_from_der(cert: concurrency::Bytes) -> Result<CertificateHandle> {
    let adapter = quinn_adapter()?;
    Ok(adapter.certificate_from_der(cert))
}
