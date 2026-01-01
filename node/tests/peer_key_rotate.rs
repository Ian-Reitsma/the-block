#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use runtime::{io::read_to_end, net::TcpStream};
use std::convert::TryInto;
use std::net::SocketAddr;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use sys::tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::net::{self, Hello, Message, Payload, PeerSet, Transport, PROTOCOL_VERSION};
use the_block::{generate_keypair, rpc::run_rpc_server, Blockchain};
use util::timeout::expect_timeout;

mod util;

fn init_env() -> sys::tempfile::TempDir {
    let dir = tempdir().unwrap();
    net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
    std::env::set_var(
        "TB_PEER_KEY_HISTORY_PATH",
        dir.path().join("key_history.log"),
    );
    std::env::remove_var("HTTP_PROXY");
    std::env::remove_var("http_proxy");
    std::env::remove_var("HTTPS_PROXY");
    std::env::remove_var("https_proxy");
    dir
}

async fn rpc(addr: &str, body: &str) -> foundation_serialization::json::Value {
    let addr: SocketAddr = addr.parse().unwrap();
    let mut stream = expect_timeout(TcpStream::connect(addr)).await.unwrap();
    let req = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
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
    foundation_serialization::json::from_str(&resp[body_idx + 4..]).unwrap()
}

#[testkit::tb_serial]
fn peer_key_rotate() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);

        let peers = PeerSet::new(Vec::new());
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
            gossip_addr: None,
            quic_addr: None,
            quic_cert: None,
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign handshake");
        peers.handle_message(msg, None, &bc);

        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            Default::default(),
            tx,
        ));
        let addr = expect_timeout(rx).await.unwrap();

        // rotate key
        let (_new_sk_bytes, new_pk_vec) = generate_keypair();
        let new_pk: [u8; 32] = new_pk_vec.as_slice().try_into().unwrap();
        let sig = sk.sign(&new_pk);
        let pk_hex = crypto_suite::hex::encode(pk);
        let new_pk_hex = crypto_suite::hex::encode(new_pk);
        let body = format!(
            "{{\"method\":\"net.key_rotate\",\"params\":{{\"peer_id\":\"{}\",\"new_key\":\"{}\",\"signature\":\"{}\"}}}}",
            pk_hex,
            new_pk_hex,
            crypto_suite::hex::encode(sig.to_bytes()),
        );
        let res = rpc(&addr, &body).await;
        let result = res
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| res.get("result"));
        let status = result
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str());
        assert_eq!(status, Some("ok"));

        // old key rejected
        let body_old = format!(
            "{{\"method\":\"net.peer_stats\",\"params\":{{\"peer_id\":\"{}\"}}}}",
            pk_hex
        );
        let val = rpc(&addr, &body_old).await;
        let has_error = val.get("error").is_some() || val.get("Error").is_some();
        assert!(has_error);

        // new key retains metrics
        let body_new = format!(
            "{{\"method\":\"net.peer_stats\",\"params\":{{\"peer_id\":\"{}\"}}}}",
            new_pk_hex
        );
        let val = rpc(&addr, &body_new).await;
        let result = val
            .get("Result")
            .and_then(|r| r.get("result"))
            .or_else(|| val.get("result"))
            .expect("peer_stats result");
        assert_eq!(result["requests"].as_u64().unwrap(), 1);

        handle.abort();
        Settlement::shutdown();
    });
}
