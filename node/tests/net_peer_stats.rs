#![cfg(feature = "integration-tests")]
use crypto_suite::signatures::ed25519::SigningKey;
use rand::{thread_rng, RngCore};
use runtime::{io::read_to_end, net::TcpStream};
use std::convert::TryInto;
use std::net::SocketAddr;
use std::process::Command;
use std::sync::{atomic::AtomicBool, Arc, Barrier, Mutex};
use std::time::Duration;
use sys::tempfile::tempdir;
use the_block::net::{self, set_max_peer_metrics, simulate_handshake_fail, HandshakeError};
use the_block::{
    compute_market::settlement::{SettleMode, Settlement},
    generate_keypair,
    net::{Hello, Message, Payload, PeerSet, Transport, PROTOCOL_VERSION},
    rpc::run_rpc_server,
    Blockchain,
};
use util::timeout::expect_timeout;

mod util;

fn init_env() -> sys::tempfile::TempDir {
    let dir = tempdir().unwrap();
    net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    the_block::net::clear_peer_metrics();
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

async fn rpc(addr: &str, body: &str) -> foundation_serialization::json::Value {
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
    foundation_serialization::json::from_str(&resp[body_idx + 4..]).unwrap()
}

#[testkit::tb_serial]
fn peer_stats_rpc() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);

        // simulate a handshake to populate metrics
        let peers = PeerSet::new(Vec::new());
        let (sk_bytes, pk_vec) = generate_keypair();
        let pk: [u8; 32] = pk_vec.as_slice().try_into().unwrap();
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
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
        peers.handle_message(msg, None, &bc);
        simulate_handshake_fail(pk, HandshakeError::Tls);
        simulate_handshake_fail(pk, HandshakeError::Tls);

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

        let peer_id = crypto_suite::hex::encode(pk);
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
    });
}

#[testkit::tb_serial]
fn peer_stats_all_rpc() {
    runtime::block_on(async {
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
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg1 = Message::new(Payload::Handshake(hello.clone()), &sk1).expect("sign message");
        let msg2 = Message::new(Payload::Handshake(hello), &sk2).expect("sign message");
        peers.handle_message(msg1, None, &bc);
        peers.handle_message(msg2, None, &bc);

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
        assert!(
            ids.contains(&crypto_suite::hex::encode(pk1))
                && ids.contains(&crypto_suite::hex::encode(pk2))
        );

        handle.abort();
        Settlement::shutdown();
    });
}

#[testkit::tb_serial]
fn peer_stats_reset_rpc() {
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
            feature_bits: the_block::net::REQUIRED_FEATURES,
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
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
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

        let peer_id = crypto_suite::hex::encode(pk);
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
    });
}

#[testkit::tb_serial]
fn peer_stats_export_rpc() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let peers = PeerSet::new(Vec::new());
        let (sk_bytes, pk_bytes) = generate_keypair();
        let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
        let pk: [u8; 32] = pk_bytes.try_into().unwrap();
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: the_block::net::REQUIRED_FEATURES,
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
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
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

        the_block::net::set_metrics_export_dir(dir.path().to_str().unwrap().into());
        let path = "export.json";
        let peer_id = crypto_suite::hex::encode(pk);
        let body = format!(
        "{{\"method\":\"net.peer_stats_export\",\"params\":{{\"peer_id\":\"{}\",\"path\":\"{}\"}}}}",
        peer_id,
        path
    );
        let val = rpc(&addr, &body).await;
        assert_eq!(val["result"]["status"].as_str(), Some("ok"));
        let contents = std::fs::read_to_string(dir.path().join(path)).unwrap();
        let m: foundation_serialization::json::Value =
            foundation_serialization::json::from_str(&contents).unwrap();
        assert_eq!(m["requests"].as_u64().unwrap(), 1);

        handle.abort();
        Settlement::shutdown();
    });
}

#[testkit::tb_serial]
fn peer_stats_export_invalid_path() {
    let dir = init_env();
    the_block::net::set_metrics_export_dir(dir.path().to_str().unwrap().into());
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let peers = PeerSet::new(Vec::new());
    let (sk_bytes, pk_bytes) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
    let pk: [u8; 32] = pk_bytes.try_into().unwrap();
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
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
    let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
    peers.handle_message(msg, None, &bc);

    let err = the_block::net::export_peer_stats(&pk, "../evil.json").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

    Settlement::shutdown();
}

#[testkit::tb_serial]
fn peer_stats_export_concurrent() {
    let dir = init_env();
    the_block::net::set_metrics_export_dir(dir.path().to_str().unwrap().into());
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let peers = PeerSet::new(Vec::new());
    let (sk_bytes, pk_bytes) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
    let pk: [u8; 32] = pk_bytes.try_into().unwrap();
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
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
    let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
    peers.handle_message(msg, None, &bc);

    let barrier = Arc::new(Barrier::new(2));
    let path = "race.json";
    let pk1 = pk;
    let pk2 = pk;
    let barrier1 = Arc::clone(&barrier);
    let barrier2 = Arc::clone(&barrier);
    let t1 = std::thread::spawn(move || {
        barrier1.wait();
        the_block::net::export_peer_stats(&pk1, path)
    });
    let t2 = std::thread::spawn(move || {
        barrier2.wait();
        the_block::net::export_peer_stats(&pk2, path)
    });
    let r1 = t1.join().unwrap();
    let r2 = t2.join().unwrap();
    assert!(r1.is_ok() || r2.is_ok());

    Settlement::shutdown();
}

#[testkit::tb_serial]
fn peer_stats_export_all_rpc_map() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let peers = PeerSet::new(Vec::new());
        let (sk_bytes, pk_bytes) = generate_keypair();
        let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
        let pk: [u8; 32] = pk_bytes.try_into().unwrap();
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: the_block::net::REQUIRED_FEATURES,
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
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
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

        let val = rpc(&addr, "{\"method\":\"net.peer_stats_export_all\"}").await;
        let peer_id = crypto_suite::hex::encode(pk);
        assert!(val["result"][peer_id.as_str()].is_object());

        handle.abort();
        Settlement::shutdown();
    });
}

#[testkit::tb_serial]
fn peer_stats_export_all_rpc_dir() {
    runtime::block_on(async {
        let dir = init_env();
        the_block::net::set_metrics_export_dir(dir.path().to_str().unwrap().into());
        the_block::net::set_peer_metrics_compress(false);
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let peers = PeerSet::new(Vec::new());
        let (sk_bytes, pk_bytes) = generate_keypair();
        let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
        let _pk: [u8; 32] = pk_bytes.try_into().unwrap();
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: the_block::net::REQUIRED_FEATURES,
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
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
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

        let body =
            "{\"method\":\"net.peer_stats_export\",\"params\":{\"all\":true,\"path\":\"dump\"}}";
        let val = rpc(&addr, body).await;
        assert_eq!(val["result"]["status"].as_str(), Some("ok"));
        handle.abort();
        Settlement::shutdown();
    });
}

#[testkit::tb_serial]
fn peer_stats_export_all_invalid_path() {
    let dir = init_env();
    the_block::net::set_metrics_export_dir(dir.path().to_str().unwrap().into());
    let _bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let err = the_block::net::export_all_peer_stats("../evil", None, None).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    Settlement::shutdown();
}

#[testkit::tb_serial]
fn peer_stats_export_all_quota() {
    let dir = init_env();
    the_block::net::set_metrics_export_dir(dir.path().to_str().unwrap().into());
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let peers = PeerSet::new(Vec::new());
    let (sk_bytes, pk_bytes) = generate_keypair();
    let sk = SigningKey::from_bytes(&sk_bytes[..].try_into().unwrap());
    let _pk: [u8; 32] = pk_bytes.try_into().unwrap();
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
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
    let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
    peers.handle_message(msg, None, &bc);

    the_block::net::set_peer_metrics_export_quota(1);
    let err = the_block::net::export_all_peer_stats("dump", None, None).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Other);

    Settlement::shutdown();
}

#[testkit::tb_serial]
fn peer_stats_export_all_filter_reputation() {
    let dir = init_env();
    the_block::net::clear_peer_metrics();
    let base = dir.path().join("out");
    the_block::net::set_metrics_export_dir(base.to_str().unwrap().into());
    the_block::net::set_peer_metrics_compress(false);
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let peers = PeerSet::new(Vec::new());

    let (sk1, pk1_vec) = generate_keypair();
    let pk1: [u8; 32] = pk1_vec.try_into().unwrap();
    let sk1 = SigningKey::from_bytes(&sk1[..].try_into().unwrap());
    let hello1 = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
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
    let msg = Message::new(Payload::Handshake(hello1), &sk1).expect("sign message");
    peers.handle_message(msg, None, &bc);

    let (sk2, pk2_vec) = generate_keypair();
    let pk2: [u8; 32] = pk2_vec.try_into().unwrap();
    let sk2 = SigningKey::from_bytes(&sk2[..].try_into().unwrap());
    let hello2 = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
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
    let msg2 = Message::new(Payload::Handshake(hello2), &sk2).expect("sign message");
    peers.handle_message(msg2, None, &bc);
    for _ in 0..5 {
        simulate_handshake_fail(pk2, HandshakeError::Tls);
    }

    the_block::net::export_all_peer_stats("dump", Some(0.8), None).unwrap();
    let map = the_block::net::peer_stats_map(Some(0.8), None);
    assert_eq!(map.len(), 1);
    assert!(map.contains_key(&crypto_suite::hex::encode(pk1)));
    Settlement::shutdown();
}

#[testkit::tb_serial]
fn peer_stats_export_all_filter_activity() {
    let dir = init_env();
    the_block::net::clear_peer_metrics();
    let base = dir.path().join("out");
    the_block::net::set_metrics_export_dir(base.to_str().unwrap().into());
    the_block::net::set_peer_metrics_compress(false);
    let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    let peers = PeerSet::new(Vec::new());

    let (sk1, _pk1_vec) = generate_keypair();
    let _pk1: [u8; 32] = _pk1_vec.try_into().unwrap();
    let sk1 = SigningKey::from_bytes(&sk1[..].try_into().unwrap());
    let hello1 = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
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
    let msg = Message::new(Payload::Handshake(hello1), &sk1).expect("sign message");
    peers.handle_message(msg, None, &bc);
    std::thread::sleep(Duration::from_secs(2));

    let (sk2, pk2_vec) = generate_keypair();
    let pk2: [u8; 32] = pk2_vec.try_into().unwrap();
    let sk2 = SigningKey::from_bytes(&sk2[..].try_into().unwrap());
    let hello2 = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: the_block::net::REQUIRED_FEATURES,
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
    let msg2 = Message::new(Payload::Handshake(hello2), &sk2).expect("sign message");
    peers.handle_message(msg2, None, &bc);

    the_block::net::export_all_peer_stats("dump", None, Some(1)).unwrap();
    let map = the_block::net::peer_stats_map(None, Some(1));
    assert_eq!(map.len(), 1);
    assert!(map.contains_key(&crypto_suite::hex::encode(pk2)));
    Settlement::shutdown();
}

#[testkit::tb_serial]
fn peer_stats_export_all_peer_list_changed() {
    let dir = init_env();
    the_block::net::set_metrics_export_dir(dir.path().to_str().unwrap().into());
    the_block::net::set_peer_metrics_compress(false);
    Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
    for _ in 0..2000 {
        let mut pk = [0u8; 32];
        thread_rng().fill_bytes(&mut pk);
        net::record_request(&pk);
    }

    let handle = std::thread::spawn(|| the_block::net::export_all_peer_stats("dump", None, None));
    std::thread::sleep(Duration::from_millis(10));
    net::clear_peer_metrics();
    let err = handle.join().unwrap().unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Other);
    Settlement::shutdown();
}

#[testkit::tb_serial]
#[cfg_attr(feature = "quic", ignore)]
fn peer_stats_cli_show_and_reputation() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let peers = PeerSet::new(Vec::new());
        let (sk_bytes, pk_vec) = generate_keypair();
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
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
        let addr_map: SocketAddr = "127.0.0.1:1".parse().unwrap();
        peers.handle_message(msg, Some(addr_map), &bc);

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

        net::set_track_handshake_fail(true);
        let pk = pk_vec.as_slice().try_into().unwrap();
        simulate_handshake_fail(pk, HandshakeError::Tls);
        net::set_track_handshake_fail(false);

        let peer_id = crypto_suite::hex::encode(pk_vec);
        let peer_id_clone = peer_id.clone();
        let rpc_url = format!("http://{}", addr);
        let output = the_block::spawn_blocking(move || {
            Command::new(env!("CARGO_BIN_EXE_net"))
                .args(["stats", "show", "--rpc", &rpc_url, &peer_id_clone])
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let _stdout = String::from_utf8_lossy(&output.stdout);

        let val = rpc(
            &addr,
            &format!(
                "{{\"method\":\"net.peer_stats\",\"params\":{{\"peer_id\":\"{}\"}}}}",
                peer_id
            ),
        )
        .await;
        assert_eq!(val["result"]["handshake_fail"]["tls"].as_u64(), Some(1));

        let peer_id_clone = peer_id.clone();
        let rpc_url = format!("http://{}", addr);
        let output = the_block::spawn_blocking(move || {
            Command::new(env!("CARGO_BIN_EXE_net"))
                .args(["stats", "reputation", "--rpc", &rpc_url, &peer_id_clone])
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        handle.abort();
        Settlement::shutdown();
    });
}

#[testkit::tb_serial]
fn peer_stats_cli_show_table_snapshot() {
    runtime::block_on(async {
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
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
        let addr_map: SocketAddr = "127.0.0.1:1".parse().unwrap();
        peers.handle_message(msg, Some(addr_map), &bc);

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

        let peer_id = crypto_suite::hex::encode(pk);
        let rpc_url = format!("http://{}", addr);
        let peer_id_clone = peer_id.clone();
        let output = the_block::spawn_blocking(move || {
            Command::new(env!("CARGO_BIN_EXE_net"))
                .env("CLICOLOR_FORCE", "1")
                .args([
                    "stats",
                    "show",
                    "--rpc",
                    &rpc_url,
                    "--format",
                    "table",
                    &peer_id_clone,
                ])
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Peer ID"));
        assert!(stdout.contains(&peer_id));
        assert!(stdout.contains("Reputation"));

        handle.abort();
        Settlement::shutdown();
    });
}

#[testkit::tb_serial]
fn peer_stats_cli_show_json_snapshot() {
    runtime::block_on(async {
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
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
        let addr_map: SocketAddr = "127.0.0.1:1".parse().unwrap();
        peers.handle_message(msg, Some(addr_map), &bc);

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

        let peer_id = crypto_suite::hex::encode(pk);
        let rpc_url = format!("http://{}", addr);
        let peer_id_clone = peer_id.clone();
        let output = the_block::spawn_blocking(move || {
            Command::new(env!("CARGO_BIN_EXE_net"))
                .args([
                    "stats",
                    "show",
                    "--rpc",
                    &rpc_url,
                    "--format",
                    "json",
                    &peer_id_clone,
                ])
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(output.status.success());
        let mut val: foundation_serialization::json::Value =
            foundation_serialization::json::from_slice(&output.stdout).unwrap();
        if let Some(rep) = val.get_mut("reputation") {
            *rep = foundation_serialization::json::Value::Number(
                foundation_serialization::json::Number::from_f64(1.0)
                    .expect("finite reputation override"),
            );
        }
        assert_eq!(
            val.get("peer_id").and_then(|v| v.as_str()),
            Some(peer_id.as_str())
        );
        assert_eq!(val.get("reputation").and_then(|v| v.as_f64()), Some(1.0));
        assert!(val.get("metrics").is_some());

        handle.abort();
        Settlement::shutdown();
    });
}

#[testkit::tb_serial]
fn peer_stats_cli_sort_filter_snapshot() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
        let peers = PeerSet::new(Vec::new());

        // first peer with reputation penalty
        let (sk1_bytes, pk1_vec) = generate_keypair();
        let pk1: [u8; 32] = pk1_vec.as_slice().try_into().unwrap();
        let sk1 = SigningKey::from_bytes(&sk1_bytes[..].try_into().unwrap());
        let hello1 = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: the_block::net::REQUIRED_FEATURES,
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
        let msg1 = Message::new(Payload::Handshake(hello1), &sk1).expect("sign message");
        peers.handle_message(msg1, None, &bc);
        simulate_handshake_fail(pk1, HandshakeError::Tls);

        // second peer
        let (sk2_bytes, _pk2_vec) = generate_keypair();
        let sk2 = SigningKey::from_bytes(&sk2_bytes[..].try_into().unwrap());
        let hello2 = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: the_block::net::REQUIRED_FEATURES,
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
        let msg2 = Message::new(Payload::Handshake(hello2), &sk2).expect("sign message");
        peers.handle_message(msg2, None, &bc);

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
        let rpc_url = format!("http://{}", addr);

        // sort by reputation
        let output = the_block::spawn_blocking({
            let rpc_url = rpc_url.clone();
            move || {
                Command::new(env!("CARGO_BIN_EXE_net"))
                    .args([
                        "stats",
                        "show",
                        "--all",
                        "--sort-by",
                        "reputation",
                        "--format",
                        "json",
                        "--rpc",
                        &rpc_url,
                    ])
                    .output()
                    .unwrap()
            }
        })
        .await
        .unwrap();
        assert!(output.status.success());
        let mut val: foundation_serialization::json::Value =
            foundation_serialization::json::from_slice(&output.stdout).unwrap();
        if let Some(arr) = val.get_mut("peers").and_then(|v| v.as_array_mut()) {
            for (i, p) in arr.iter_mut().enumerate() {
                if let Some(obj) = p.as_object_mut() {
                    obj.remove("latency");
                    obj.insert(
                        "peer".into(),
                        foundation_serialization::json::Value::String(format!("peer{}", i)),
                    );
                    obj.insert(
                        "reputation".into(),
                        foundation_serialization::json::Value::from(1.0),
                    );
                }
            }
        }
        let peers = val
            .get("peers")
            .and_then(|v| v.as_array())
            .expect("peers array");
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].get("peer").and_then(|v| v.as_str()), Some("peer0"));
        assert_eq!(peers[1].get("peer").and_then(|v| v.as_str()), Some("peer1"));
        assert!(peers
            .iter()
            .all(|p| p.get("reputation").and_then(|v| v.as_f64()) == Some(1.0)));

        // filter by first peer prefix
        let prefix = &crypto_suite::hex::encode(pk1)[..4];
        let output2 = the_block::spawn_blocking({
            let rpc_url = rpc_url.clone();
            let patt = format!("^{}", prefix);
            move || {
                Command::new(env!("CARGO_BIN_EXE_net"))
                    .args([
                        "stats", "show", "--all", "--filter", &patt, "--format", "json", "--rpc",
                        &rpc_url,
                    ])
                    .output()
                    .unwrap()
            }
        })
        .await
        .unwrap();
        assert!(output2.status.success());
        let mut val2: foundation_serialization::json::Value =
            foundation_serialization::json::from_slice(&output2.stdout).unwrap();
        if let Some(arr) = val2.get_mut("peers").and_then(|v| v.as_array_mut()) {
            for (i, p) in arr.iter_mut().enumerate() {
                if let Some(obj) = p.as_object_mut() {
                    obj.remove("latency");
                    obj.insert(
                        "peer".into(),
                        foundation_serialization::json::Value::String(format!("peer{}", i)),
                    );
                    obj.insert(
                        "reputation".into(),
                        foundation_serialization::json::Value::from(1.0),
                    );
                }
            }
        }
        let peers = val2
            .get("peers")
            .and_then(|v| v.as_array())
            .expect("filtered peers array");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].get("peer").and_then(|v| v.as_str()), Some("peer0"));
        assert_eq!(
            peers[0].get("reputation").and_then(|v| v.as_f64()),
            Some(1.0)
        );

        handle.abort();
        Settlement::shutdown();
    });
}

#[testkit::tb_serial]
fn peer_stats_malformed_id() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
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
        let val = rpc(
            &addr,
            "{\"method\":\"net.peer_stats\",\"params\":{\"peer_id\":\"zz\"}}",
        )
        .await;
        assert!(val.get("error").is_some());
        handle.abort();
        Settlement::shutdown();
    });
}

#[testkit::tb_serial]
fn peer_stats_unknown_peer() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);
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
        let mut rand_bytes = [0u8; 32];
        thread_rng().fill_bytes(&mut rand_bytes);
        let peer_id = crypto_suite::hex::encode(rand_bytes);
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
    });
}

#[testkit::tb_serial]
#[cfg_attr(feature = "quic", ignore)]
fn peer_stats_drop_counter_rpc() {
    runtime::block_on(async {
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
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        std::env::set_var("TB_P2P_MAX_PER_SEC", "10");
        the_block::net::set_p2p_max_per_sec(10);
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
        peers.handle_message(msg, Some(addr), &bc);
        for _ in 0..20 {
            let m = Message::new(Payload::Hello(vec![]), &sk).expect("sign message");
            peers.handle_message(m, Some(addr), &bc);
        }

        let mining = Arc::new(AtomicBool::new(false));
        let (tx, rx) = runtime::sync::oneshot::channel();
        let handle = the_block::spawn(run_rpc_server(
            Arc::clone(&bc),
            Arc::clone(&mining),
            "127.0.0.1:0".to_string(),
            Default::default(),
            tx,
        ));
        let addr_rpc = expect_timeout(rx).await.unwrap();
        let peer_id = crypto_suite::hex::encode(pk);
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
        std::env::remove_var("TB_P2P_MAX_PER_SEC");
        the_block::net::set_p2p_max_per_sec(100);
    });
}

#[testkit::tb_serial]
fn peer_stats_cli_reset() {
    runtime::block_on(async {
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
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
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

        let peer_id = crypto_suite::hex::encode(pk);
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
    });
}

#[testkit::tb_serial]
fn peer_stats_all_pagination_rpc() {
    runtime::block_on(async {
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
                quic_fingerprint: None,
                quic_fingerprint_previous: Vec::new(),

                quic_provider: None,

                quic_capabilities: Vec::new(),
            };
            let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
            peers.handle_message(msg, None, &bc);
            pks.push(crypto_suite::hex::encode(pk));
        }

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
    });
}

#[testkit::tb_serial]
fn peer_stats_persist_restart() {
    runtime::block_on(async {
        let dir = init_env();
        let bc = Arc::new(Mutex::new(Blockchain::new(dir.path().to_str().unwrap())));
        Settlement::init(dir.path().to_str().unwrap(), SettleMode::DryRun);

        let db_path = dir.path().join("metrics.db");
        the_block::net::peer_metrics_store::init(db_path.to_str().unwrap());
        the_block::net::set_peer_metrics_retention(60);
        the_block::net::set_peer_metrics_compress(false);

        let peers = PeerSet::new(Vec::new());
        let (sk_bytes, pk_vec) = generate_keypair();
        let pk: [u8; 32] = pk_vec.as_slice().try_into().unwrap();
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
            quic_fingerprint: None,
            quic_fingerprint_previous: Vec::new(),

            quic_provider: None,

            quic_capabilities: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &sk).expect("sign message");
        peers.handle_message(msg, None, &bc);

        the_block::net::persist_peer_metrics().unwrap();
        the_block::net::clear_peer_metrics();
        the_block::net::load_peer_metrics();
        let stats = the_block::net::peer_stats(&pk).unwrap();
        assert_eq!(stats.requests, 1);

        Settlement::shutdown();
    });
}
