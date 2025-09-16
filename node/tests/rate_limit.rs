use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use serial_test::serial;

use ed25519_dalek::SigningKey;
use rand::{thread_rng, RngCore};
use the_block::net::{
    peer_stats, record_ip_drop, set_max_peer_metrics, DropReason, Hello, Message, Payload, PeerSet,
    Transport, LOCAL_FEATURES, PROTOCOL_VERSION,
};
use the_block::Blockchain;

#[test]
fn ip_drop_increments_metric() {
    let ip: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    record_ip_drop(&ip);
    #[cfg(feature = "telemetry")]
    {
        use the_block::telemetry::PEER_DROP_TOTAL;
        let id = ip.to_string();
        assert_eq!(
            PEER_DROP_TOTAL
                .with_label_values(&[id.as_str(), "duplicate"])
                .get(),
            1
        );
    }
}

#[test]
#[serial]
fn rate_limit_drop_records_reason() {
    // Lower the per-second threshold so we can reliably trigger a drop without
    // burning the full default quota. Environment variables are read once on
    // first access, so set it before instantiating any peer structures.
    std::env::set_var("TB_P2P_MAX_PER_SEC", "10");
    the_block::net::set_p2p_max_per_sec(10);
    let peers = PeerSet::new(vec![]);
    let chain = Arc::new(Mutex::new(Blockchain::default()));

    let mut seed = [0u8; 32];
    thread_rng().fill_bytes(&mut seed);
    let key = SigningKey::from_bytes(&seed);
    let pk = key.verifying_key().to_bytes();
    let addr: SocketAddr = "127.0.0.1:9001".parse().unwrap();
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: LOCAL_FEATURES,
        agent: "test".into(),
        nonce: 1,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
    };
    let msg = Message::new(Payload::Handshake(hello), &key);
    peers.handle_message(msg, Some(addr), &chain);

    // Send enough messages to exceed the lowered rate limit (10 per sec)
    for _ in 0..20 {
        let msg = Message::new(Payload::Hello(vec![]), &key);
        peers.handle_message(msg, Some(addr), &chain);
    }

    let stats = peer_stats(&pk).unwrap();
    assert!(
        stats
            .drops
            .get(&DropReason::RateLimit)
            .copied()
            .unwrap_or(0)
            >= 1
    );
    #[cfg(feature = "telemetry")]
    {
        use the_block::telemetry::{PEER_DROP_TOTAL, PEER_METRICS_ACTIVE};
        let id = hex::encode(pk);
        assert!(
            PEER_DROP_TOTAL
                .with_label_values(&[id.as_str(), "rate_limit"])
                .get()
                >= 1
        );
        assert!(PEER_METRICS_ACTIVE.get() >= 1);
    }

    // Avoid leaking the overridden rate limit to other tests in this binary.
    std::env::remove_var("TB_P2P_MAX_PER_SEC");
    the_block::net::set_p2p_max_per_sec(100);
}

#[test]
fn evicts_least_recently_used_peer() {
    set_max_peer_metrics(2);
    let peers = PeerSet::new(vec![]);
    let chain = Arc::new(Mutex::new(Blockchain::default()));

    fn handshake(
        peers: &PeerSet,
        chain: &Arc<Mutex<Blockchain>>,
        port: u16,
    ) -> ([u8; 32], SigningKey, SocketAddr) {
        let mut seed = [0u8; 32];
        thread_rng().fill_bytes(&mut seed);
        let key = SigningKey::from_bytes(&seed);
        let pk = key.verifying_key().to_bytes();
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        let hello = Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: LOCAL_FEATURES,
            agent: "test".into(),
            nonce: port as u64,
            transport: Transport::Tcp,
            quic_addr: None,
            quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
        };
        let msg = Message::new(Payload::Handshake(hello), &key);
        peers.handle_message(msg, Some(addr), chain);
        (pk, key, addr)
    }

    let (pk1, k1, addr1) = handshake(&peers, &chain, 8001);
    let (pk2, _k2, _addr2) = handshake(&peers, &chain, 8002);

    // touch pk1 to mark as recently used
    let msg = Message::new(Payload::Hello(vec![]), &k1);
    peers.handle_message(msg, Some(addr1), &chain);

    let (pk3, _k3, _addr3) = handshake(&peers, &chain, 8003);

    assert!(peer_stats(&pk1).is_some());
    assert!(peer_stats(&pk2).is_none());
    assert!(peer_stats(&pk3).is_some());
}

#[test]
#[serial]
fn reputation_decreases_on_rate_limit() {
    let peers = PeerSet::new(vec![]);
    let chain = Arc::new(Mutex::new(Blockchain::default()));
    let mut seed = [0u8; 32];
    thread_rng().fill_bytes(&mut seed);
    let key = SigningKey::from_bytes(&seed);
    let pk = key.verifying_key().to_bytes();
    let addr: SocketAddr = "127.0.0.1:9102".parse().unwrap();
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: LOCAL_FEATURES,
        agent: "test".into(),
        nonce: 2,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
        quic_fingerprint: None,
        quic_fingerprint_previous: Vec::new(),
    };
    let msg = Message::new(Payload::Handshake(hello), &key);
    peers.handle_message(msg, Some(addr), &chain);
    for _ in 0..101 {
        let m = Message::new(Payload::Hello(vec![]), &key);
        peers.handle_message(m, Some(addr), &chain);
    }
    let rep = peer_stats(&pk).unwrap().reputation.score;
    assert!(rep < 1.0);
}
