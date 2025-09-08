use ed25519_dalek::SigningKey;
use rand::{thread_rng, RngCore};
use serial_test::serial;
use std::convert::TryInto;
use std::process::Command;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use tempfile::tempdir;
use the_block::net::{self, set_max_peer_metrics};
use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    generate_keypair,
    net::{Hello, Message, Payload, PeerSet, Transport, PROTOCOL_VERSION},
    rpc::run_rpc_server,
    Blockchain,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use util::timeout::expect_timeout;

mod util;

fn init_env() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers.txt"));
    // Ensure rate-limit tests trigger drops by tightening default limits.
    std::env::set_var("TB_P2P_SHARD_BURST", "100");
    std::env::set_var("TB_P2P_SHARD_RATE", "100");
    // Avoid proxy interference with local RPC calls in tests.
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
async fn peer_stats_rpc() {
    let dir = init_env();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);

    // simulate a handshake to populate metrics
    let peers = PeerSet::new(Vec::new());
    let (sk_bytes, pk) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
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

    let peer_id = hex::encode(pk);
    let val = rpc(
        &addr,
        &format!(
            "{{\"method\":\"net.peer_stats\",\"params\":{{\"peer_id\":\"{}\"}}}}",
            peer_id
        ),
    )
    .await;
    assert_eq!(val["result"]["requests"].as_u64().unwrap(), 1);
    assert_eq!(val["result"]["bytes_sent"].as_u64().unwrap(), 0);
    assert!(val["result"]["drops"].as_object().unwrap().is_empty());

    handle.abort();
    Settlement::shutdown();
    set_max_peer_metrics(1024);
}

#[tokio::test]
#[serial]
async fn peer_stats_all_rpc() {
    let dir = init_env();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);

    // simulate two handshakes to populate metrics
    let peers = PeerSet::new(Vec::new());
    let (sk1_bytes, pk1) = generate_keypair();
    let (sk2_bytes, pk2) = generate_keypair();
    let sk1 = SigningKey::from_bytes(&sk1_bytes[..].try_into().unwrap());
    let sk2 = SigningKey::from_bytes(&sk2_bytes[..].try_into().unwrap());
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
    };
    let msg1 = Message::new(Payload::Handshake(hello.clone()), &sk1);
    let msg2 = Message::new(Payload::Handshake(hello), &sk2);
    peers.handle_message(msg1, None, &bc);
    peers.handle_message(msg2, None, &bc);

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

    let val = rpc(
        &addr,
        "{\"method\":\"net.peer_stats_all\",\"params\":{\"offset\":0,\"limit\":10}}",
    )
    .await;
    let arr = val["result"].as_array().unwrap();
    let ids: Vec<String> = arr
        .iter()
        .map(|e| e["peer_id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&hex::encode(pk1)) && ids.contains(&hex::encode(pk2)));

    handle.abort();
    Settlement::shutdown();
}

#[tokio::test]
#[serial]
async fn peer_stats_reset_rpc() {
    let dir = init_env();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);

    let peers = PeerSet::new(Vec::new());
    let (sk_bytes, pk) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
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

    let peer_id = hex::encode(pk);
    let _ = rpc(
        &addr,
        &format!(
            "{{\"method\":\"net.peer_stats_reset\",\"params\":{{\"peer_id\":\"{}\"}}}}",
            peer_id
        ),
    )
    .await;

    let val = rpc(
        &addr,
        &format!(
            "{{\"method\":\"net.peer_stats\",\"params\":{{\"peer_id\":\"{}\"}}}}",
            peer_id
        ),
    )
    .await;
    assert_eq!(val["result"]["requests"].as_u64().unwrap(), 0);
    assert_eq!(val["result"]["bytes_sent"].as_u64().unwrap(), 0);
    assert!(val["result"]["drops"].as_object().unwrap().is_empty());

    handle.abort();
    Settlement::shutdown();
}

#[tokio::test]
#[serial]
async fn peer_stats_export_rpc() {
    let dir = init_env();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let peers = PeerSet::new(Vec::new());
    let (sk_bytes, pk) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
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

    let path = dir.path().join("export.json");
    let peer_id = hex::encode(pk);
    let body = format!(
        "{{\"method\":\"net.peer_stats_export\",\"params\":{{\"peer_id\":\"{}\",\"path\":\"{}\"}}}}",
        peer_id,
        path.display()
    );
    let val = rpc(&addr, &body).await;
    assert_eq!(val["result"]["status"].as_str(), Some("ok"));
    let contents = std::fs::read_to_string(&path).unwrap();
    let m: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(m["requests"].as_u64().unwrap(), 1);

    handle.abort();
    Settlement::shutdown();
}

#[tokio::test]
#[serial]
#[cfg_attr(feature = "quic", ignore)]
async fn peer_stats_cli_show_and_reputation() {
    let dir = init_env();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let peers = PeerSet::new(Vec::new());
    let (sk_bytes, pk) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
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

    let peer_id = hex::encode(pk);
    let output = Command::new(env!("CARGO_BIN_EXE_net"))
        .args([
            "stats",
            "show",
            "--rpc",
            &format!("http://{}", addr),
            &peer_id,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _stdout = String::from_utf8_lossy(&output.stdout);

    let output = Command::new(env!("CARGO_BIN_EXE_net"))
        .args([
            "stats",
            "reputation",
            "--rpc",
            &format!("http://{}", addr),
            &peer_id,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    handle.abort();
    Settlement::shutdown();
}

#[tokio::test]
#[serial]
async fn peer_stats_malformed_id() {
    let dir = init_env();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
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
    let val = rpc(
        &addr,
        "{\"method\":\"net.peer_stats\",\"params\":{\"peer_id\":\"zz\"}}",
    )
    .await;
    assert!(val.get("error").is_some());
    handle.abort();
    Settlement::shutdown();
}

#[tokio::test]
#[serial]
async fn peer_stats_unknown_peer() {
    let dir = init_env();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
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
    let mut rand_bytes = [0u8; 32];
    thread_rng().fill_bytes(&mut rand_bytes);
    let peer_id = hex::encode(rand_bytes);
    let val = rpc(
        &addr,
        &format!(
            "{{\"method\":\"net.peer_stats\",\"params\":{{\"peer_id\":\"{}\"}}}}",
            peer_id
        ),
    )
    .await;
    assert!(val.get("error").is_some());
    handle.abort();
    Settlement::shutdown();
}

#[tokio::test]
#[serial]
#[cfg_attr(feature = "quic", ignore)]
async fn peer_stats_drop_counter_rpc() {
    let dir = init_env();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);

    let peers = PeerSet::new(Vec::new());
    let (sk_bytes, pk) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
    let addr: std::net::SocketAddr = "127.0.0.1:9010".parse().unwrap();
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
    };
    let msg = Message::new(Payload::Handshake(hello), &sk);
    peers.handle_message(msg, Some(addr), &bc);

    for _ in 0..200 {
        let m = Message::new(Payload::Hello(vec![]), &sk);
        peers.handle_message(m, Some(addr), &bc);
    }

    let mining = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(run_rpc_server(
        Arc::clone(&bc),
        Arc::clone(&mining),
        "127.0.0.1:0".to_string(),
        Default::default(),
        tx,
    ));
    let addr_rpc = expect_timeout(rx).await.unwrap();
    let peer_id = hex::encode(pk);
    let val = rpc(
        &addr_rpc,
        &format!(
            "{{\"method\":\"net.peer_stats\",\"params\":{{\"peer_id\":\"{}\"}}}}",
            peer_id
        ),
    )
    .await;
    let drops = &val["result"]["drops"];
    let count = drops
        .get("rate_limit")
        .or_else(|| drops.get("other"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(count >= 1);
    handle.abort();
    Settlement::shutdown();
}

#[tokio::test]
#[serial]
async fn peer_stats_cli_reset() {
    let dir = init_env();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let peers = PeerSet::new(Vec::new());
    let (sk_bytes, pk) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
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

    let peer_id = hex::encode(pk);
    let output = Command::new(env!("CARGO_BIN_EXE_net"))
        .args([
            "stats",
            "reset",
            &peer_id,
            "--rpc",
            &format!("http://{}", addr),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    handle.abort();
    Settlement::shutdown();
}

#[tokio::test]
#[serial]
async fn peer_stats_all_pagination_rpc() {
    let dir = tempdir().unwrap();
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    set_max_peer_metrics(10);

    let peers = PeerSet::new(Vec::new());
    let mut pks: Vec<String> = Vec::new();
    for _ in 0..3 {
        let (sk_bytes, pk) = generate_keypair();
        let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: the_block::net::REQUIRED_FEATURES,
            agent: "test".into(),
            nonce: 0,
            transport: Transport::Tcp,
            quic_addr: None,
            quic_cert: None,
        };
        let msg = Message::new(Payload::Handshake(hello), &sk);
        peers.handle_message(msg, None, &bc);
        pks.push(hex::encode(pk));
    }

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
    let val = rpc(
        &addr,
        "{\"method\":\"net.peer_stats_all\",\"params\":{\"offset\":1,\"limit\":1}}",
    )
    .await;
    let arr = val["result"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["peer_id"].as_str().unwrap(), pks[1]);
    handle.abort();
    Settlement::shutdown();
}
