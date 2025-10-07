#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use runtime::{io::read_to_end, net::TcpStream};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::net::{self, Hello, Message, Payload, PROTOCOL_VERSION};
use the_block::p2p::handshake::Transport;
use the_block::{generate_keypair, rpc::run_rpc_server, Blockchain};
use util::timeout::expect_timeout;

mod util;

fn init_env() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
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
        let mut stream = expect_timeout(TcpStream::connect(addr)).await.unwrap();
        let req = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        expect_timeout(stream.write_all(req.as_bytes()))
            .await
            .unwrap();
        let mut resp = Vec::new();
        expect_timeout(read_to_end(&mut stream, &mut resp))
            .await
            .unwrap();
        let resp = String::from_utf8(resp).unwrap();
        let body_idx = resp.find("\r\n\r\n").unwrap();
        serde_json::from_str(&resp[body_idx + 4..]).unwrap()
    })
}

#[testkit::tb_serial]
fn rpc_reports_handshake_failures() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let peers = net::PeerSet::new(Vec::new());
        let (sk_bytes, pk_vec) = generate_keypair();
        let pk: [u8; 32] = pk_vec.as_slice().try_into().unwrap();
        let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: net::REQUIRED_FEATURES,
            agent: "test".into(),
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
        peers.handle_message(msg, None, &bc);

        // simulate failure and expose via RPC
        net::simulate_handshake_fail(pk, net::HandshakeError::Tls);

        let mining = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            Default::default(),
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();
        let res = rpc(&addr, "{\"method\":\"net.handshake_failures\"}").await;
        assert!(res["result"]["failures"].as_array().unwrap().len() >= 1);
        handle.abort();
        Settlement::shutdown();
    });
}
