use concurrency::Bytes;
use crypto_suite::signatures::ed25519::SigningKey;
use std::net::SocketAddr;
use std::sync::Once;
use std::time::{Duration, Instant};
use sys::tempfile as temp;
use testkit::tb_prop_test;
use the_block::gossip::config::GossipConfig;
use the_block::gossip::relay::Relay;
use the_block::net::{Message, Payload, Transport};
use the_block::simple_db::{self, EngineConfig, EngineKind};

static MEMORY_DB: Once = Once::new();

fn relay_with_config(cfg: GossipConfig) -> (Relay, temp::TempDir) {
    MEMORY_DB.call_once(|| {
        let mut config = EngineConfig::default();
        config.default_engine = EngineKind::Memory;
        simple_db::configure_engines(config);
    });
    let dir = temp::tempdir().expect("tempdir");
    let path = dir.path().join("gossip_relay_store");
    let mut cfg = cfg;
    cfg.shard_store_path = path.to_string_lossy().into_owned();
    let relay = Relay::new(cfg);
    (relay, dir)
}

tb_prop_test!(dedup_entries_expire, |runner| {
    runner
        .add_case("default ttl", || {
            let mut cfg = GossipConfig::default();
            cfg.ttl_ms = 500;
            cfg.dedup_capacity = 128;
            cfg.min_fanout = 2;
            cfg.base_fanout = 3;
            cfg.max_fanout = 6;
            let (relay, _dir) = relay_with_config(cfg);
            let sk = SigningKey::from_bytes(&[7u8; 32]);
            let msg = Message::new(Payload::Hello(vec![]), &sk).expect("sign hello");
            let start = Instant::now();
            assert!(relay.should_process_at(&msg, start));
            let before = start + Duration::from_millis(499);
            assert!(!relay.should_process_at(&msg, before));
            let after = start + Duration::from_millis(501);
            assert!(relay.should_process_at(&msg, after));
        })
        .expect("register deterministic case");
});

tb_prop_test!(fanout_respects_configuration, |runner| {
    runner
        .add_case("fanout bounds coverage", || {
            const CASES: &[(usize, usize, usize, usize)] = &[(2, 1, 1, 4), (5, 2, 2, 10)];
            for &(min, base_delta, max_delta, peers_count) in CASES {
                let base = min + base_delta;
                let max = base + max_delta;
                let mut cfg = GossipConfig::default();
                cfg.ttl_ms = 5;
                cfg.dedup_capacity = 256;
                cfg.min_fanout = min;
                cfg.base_fanout = base;
                cfg.max_fanout = max;
                let (relay, _dir) = relay_with_config(cfg);
                let sk = SigningKey::from_bytes(&[9u8; 32]);
                let msg = Message::new(Payload::Hello(vec![]), &sk).expect("sign hello");
                let peers: Vec<(SocketAddr, Transport, Option<Bytes>)> = (0..peers_count)
                    .map(|i| {
                        (
                            format!("127.0.0.1:{}", 16000 + i).parse().unwrap(),
                            Transport::Tcp,
                            None,
                        )
                    })
                    .collect();
                for (idx, (addr, _, _)) in peers.iter().enumerate() {
                    let peer = the_block::net::overlay_peer_from_bytes(&[(idx as u8) + 1; 32])
                        .expect("peer id");
                    the_block::net::peer::inject_addr_mapping_for_tests(*addr, peer);
                }
                let mut delivered = 0usize;
                relay.broadcast_with(&msg, &peers, |_, _| delivered += 1);
                let max_allowed = max.min(peers_count);
                let min_expected = min.min(peers_count.max(1));
                assert!(delivered <= max_allowed);
                assert!(delivered >= min_expected);
            }
        })
        .expect("register fanout bounds case");
});
