#![cfg(feature = "integration-tests")]
#![cfg(feature = "quic")]

use crypto_suite::signatures::ed25519::SigningKey;
use hex;
use runtime::{io::read_to_end, net::TcpStream};
use serial_test::serial;
use std::convert::TryInto;
use std::net::SocketAddr;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::generate_keypair;
use the_block::net::{self, Message, Payload, PeerSet, Transport, PROTOCOL_VERSION};
use the_block::p2p::handshake::Hello;
use the_block::rpc::run_rpc_server;
use the_block::Blockchain;

mod util;

fn init_env() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    the_block::net::clear_peer_metrics();
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
    std::env::remove_var("HTTP_PROXY");
    std::env::remove_var("http_proxy");
    std::env::remove_var("HTTPS_PROXY");
    std::env::remove_var("https_proxy");
    dir
}

fn rpc(addr: &str, body: &str) -> serde_json::Value {
    runtime::block_on(async {
        let addr: SocketAddr = addr.parse().unwrap();
        let mut stream = util::timeout::expect_timeout(TcpStream::connect(addr))
            .await
            .unwrap();
        let req = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        util::timeout::expect_timeout(stream.write_all(req.as_bytes()))
            .await
            .unwrap();
        let mut resp = Vec::new();
        util::timeout::expect_timeout(read_to_end(&mut stream, &mut resp))
            .await
            .unwrap();
        let resp = String::from_utf8(resp).unwrap();
        let body_idx = resp.find("\r\n\r\n").unwrap();
        serde_json::from_str(&resp[body_idx + 4..]).unwrap()
    })
}

#[test]
#[serial]
fn quic_stats_rpc() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);

        let peers = PeerSet::new(Vec::new());
        let (sk_bytes, pk_vec) = generate_keypair();
        let pk: [u8; 32] = pk_vec.as_slice().try_into().unwrap();
        let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());

        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let (server_ep, cert) = net::quic::listen(addr).await.unwrap();
        let listen_addr = server_ep.local_addr().unwrap();

        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: net::REQUIRED_FEATURES,
            agent: "quic-stats-test".into(),
            nonce: 0,
            transport: Transport::Tcp,
            quic_addr: None,
            quic_cert: None,
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sk);
        peers.handle_message(msg, Some(listen_addr), &bc);

        let (accept_tx, accept_rx) = runtime::sync::oneshot::channel();
        let ep = server_ep.clone();
        the_block::spawn(async move {
            if let Some(conn) = ep.accept().await {
                let connection = conn.await.unwrap();
                accept_tx.send(()).ok();
                connection.closed().await;
            } else {
                accept_tx.send(()).ok();
            }
        });

        let conn = net::quic::get_connection(listen_addr, cert.clone())
            .await
            .unwrap();
        net::quic::get_connection(listen_addr, cert.clone())
            .await
            .unwrap();
        conn.close(0u32.into(), b"done");
        accept_rx.await.unwrap();
        server_ep.wait_idle().await;

        net::simulate_handshake_fail(pk, net::HandshakeError::Tls);

        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            Default::default(),
            tx,
        ));
        let addr = util::timeout::expect_timeout(rx).await.unwrap();

        let response = rpc(&addr, "{\"method\":\"net.quic_stats\",\"params\":{}}").await;
        let result = response["result"].as_array().unwrap();
        assert!(!result.is_empty());
        let peer_hex = hex::encode(pk);
        let entry = result
            .iter()
            .find(|v| v["peer_id"].as_str() == Some(peer_hex.as_str()))
            .expect("peer stats missing");
        assert!(entry["endpoint_reuse"].as_u64().unwrap() >= 1);
        assert!(entry["handshake_failures"].as_u64().unwrap() >= 1);
        assert!(entry["last_updated"].as_u64().unwrap() > 0);

        handle.abort();
        Settlement::shutdown();
    });
}
