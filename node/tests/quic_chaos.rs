#![cfg(all(feature = "integration-tests", feature = "s2n-quic"))]

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use runtime::sync::oneshot;
use sys::tempfile::{tempdir, TempDir};
use the_block::net::transport_quic as s2n_transport;
use transport::{Config as TransportConfig, ProviderKind};

struct S2nTransportGuard {
    _dir: TempDir,
}

impl S2nTransportGuard {
    fn install() -> Self {
        let dir = tempdir().expect("tempdir");
        let cert_store = dir.path().join("cert_store.json");
        let peer_store = dir.path().join("peer_store.json");
        let net_key = dir.path().join("net_key");
        std::env::set_var("TB_NET_CERT_STORE_PATH", &cert_store);
        std::env::set_var("TB_PEER_CERT_CACHE_PATH", &peer_store);
        std::env::set_var("TB_NET_KEY_PATH", &net_key);
        let mut cfg = TransportConfig::default();
        cfg.provider = ProviderKind::S2nQuic;
        cfg.certificate_cache = Some(cert_store);
        the_block::net::configure_transport(&cfg).expect("configure transport");
        Self { _dir: dir }
    }
}

impl Drop for S2nTransportGuard {
    fn drop(&mut self) {
        std::env::remove_var("TB_NET_CERT_STORE_PATH");
        std::env::remove_var("TB_PEER_CERT_CACHE_PATH");
        std::env::remove_var("TB_NET_KEY_PATH");
        let _ = the_block::net::configure_transport(&TransportConfig::default());
    }
}

#[testkit::tb_serial]
fn s2n_handshake_recovers_from_dropped_packets() {
    runtime::block_on(async {
        let _guard = S2nTransportGuard::install();
        let listener = s2n_transport::start_server("127.0.0.1:0".parse().unwrap())
            .await
            .expect("start server");
        let server = listener.into_s2n().expect("s2n listener unavailable");
        let server_addr = server.local_addr().expect("server addr");

        let proxy_socket = runtime::net::UdpSocket::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind proxy");
        let proxy_addr = proxy_socket.local_addr().expect("proxy addr");

        let proxy_task = the_block::spawn(proxy_loop(proxy_socket, server_addr));

        let (done_tx, done_rx) = oneshot::channel();
        let acceptor = Arc::clone(&server);
        the_block::spawn(async move {
            if let Some(connecting) = acceptor.accept().await {
                let _ = connecting.await;
            }
            let _ = done_tx.send(());
        });

        s2n_transport::connect(proxy_addr)
            .await
            .expect("client connect succeeds after retries");

        done_rx.await.expect("server completed handshake");
        proxy_task
            .await
            .expect("proxy task join")
            .expect("proxy loop");
    });
}

async fn proxy_loop(
    mut socket: runtime::net::UdpSocket,
    server_addr: SocketAddr,
) -> io::Result<()> {
    let mut buf = [0u8; 64];
    let mut client_addr: Option<SocketAddr> = None;
    let mut dropped = false;
    loop {
        let (len, src) = socket.recv_from(&mut buf).await?;
        if src == server_addr {
            if let Some(client) = client_addr {
                socket.send_to(&buf[..len], client).await?;
                return Ok(());
            }
            continue;
        }
        client_addr = Some(src);
        if !dropped {
            dropped = true;
            continue;
        }
        socket.send_to(&buf[..len], server_addr).await?;
    }
}
