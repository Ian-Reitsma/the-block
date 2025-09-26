use crypto_suite::signatures::ed25519::SigningKey;
use proptest::prelude::*;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tempfile::tempdir;
use the_block::gossip::config::GossipConfig;
use the_block::gossip::relay::Relay;
use the_block::net::{Message, Payload, Transport};

fn relay_with_config(cfg: GossipConfig) -> (Relay, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("gossip_relay_store");
    let mut cfg = cfg;
    cfg.shard_store_path = path.to_string_lossy().into_owned();
    let relay = Relay::new(cfg);
    (relay, dir)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 8, failure_persistence: None, .. ProptestConfig::default() })]
    #[test]
    fn dedup_entries_expire(ttl_ms in 1u64..50) {
        let mut cfg = GossipConfig::default();
        cfg.ttl_ms = ttl_ms;
        cfg.dedup_capacity = 128;
        cfg.min_fanout = 2;
        cfg.base_fanout = 3.max(cfg.min_fanout);
        cfg.max_fanout = cfg.base_fanout + 3;
        let (relay, _dir) = relay_with_config(cfg);
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let msg = Message::new(Payload::Hello(vec![]), &sk);
        let start = Instant::now();
        prop_assert!(relay.should_process_at(&msg, start));
        let before = start + Duration::from_millis(ttl_ms.saturating_sub(1));
        prop_assert!(!relay.should_process_at(&msg, before));
        let after = start + Duration::from_millis(ttl_ms.saturating_add(1));
        prop_assert!(relay.should_process_at(&msg, after));
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 12, failure_persistence: None, .. ProptestConfig::default() })]
    #[test]
    fn fanout_respects_configuration(
        min in 1usize..5,
        base_delta in 0usize..4,
        max_delta in 0usize..4,
        peers_count in 1usize..18,
    ) {
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
        let msg = Message::new(Payload::Hello(vec![]), &sk);
        let peers: Vec<(SocketAddr, Transport, Option<Vec<u8>>)> = (0..peers_count)
            .map(|i| {
                (
                    format!("127.0.0.1:{}", 16000 + i).parse().unwrap(),
                    Transport::Tcp,
                    None,
                )
            })
            .collect();
        let mut delivered = 0usize;
        relay.broadcast_with(&msg, &peers, |_, _| delivered += 1);
        let max_allowed = max.min(peers_count);
        let min_expected = min.min(peers_count.max(1));
        prop_assert!(delivered <= max_allowed);
        prop_assert!(delivered >= min_expected);
    }
}
