#![cfg(feature = "integration-tests")]
use ed25519_dalek::{Signer, SigningKey};
use hex;
use serial_test::serial;
use std::convert::TryInto;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use tempfile::tempdir;
use the_block::compute_market::settlement::{SettleMode, Settlement};
use the_block::net::{self, Hello, Message, Payload, PeerSet, Transport, PROTOCOL_VERSION};
use the_block::{generate_keypair, rpc::run_rpc_server, Blockchain};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use util::timeout::expect_timeout;

mod util;

fn init_env() -> tempfile::TempDir {
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

async fn rpc(addr: &str, body: &str) -> serde_json::Value {
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
    expect_timeout(stream.read_to_end(&mut resp)).await.unwrap();
    let resp = String::from_utf8(resp).unwrap();
    let body_idx = resp.find("\r\n\r\n").unwrap();
    serde_json::from_str(&resp[body_idx + 4..]).unwrap()
}

#[tokio::test]
#[serial]
async fn peer_key_rotate() {
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
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
    };
    let msg = Message::new(Payload::Handshake(hello), &sk);
    peers.handle_message(msg, None, &bc);

    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(run_rpc_server(
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
    let body = format!(
        "{{\"method\":\"net.key_rotate\",\"params\":{{\"peer_id\":\"{}\",\"new_key\":\"{}\",\"signature\":\"{}\"}}}}",
        hex::encode(pk),
        hex::encode(new_pk),
        hex::encode(sig.to_bytes()),
    );
    let res = rpc(&addr, &body).await;
    assert_eq!(res["result"]["status"], "ok");

    // old key rejected
    let body_old = format!(
        "{{\"method\":\"net.peer_stats\",\"params\":{{\"peer_id\":\"{}\"}}}}",
        hex::encode(pk)
    );
    let val = rpc(&addr, &body_old).await;
    assert!(val.get("error").is_some());

    // new key retains metrics
    let body_new = format!(
        "{{\"method\":\"net.peer_stats\",\"params\":{{\"peer_id\":\"{}\"}}}}",
        hex::encode(new_pk)
    );
    let val = rpc(&addr, &body_new).await;
    assert_eq!(val["result"]["requests"].as_u64().unwrap(), 1);

    handle.abort();
    Settlement::shutdown();
}
