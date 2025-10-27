#![cfg(feature = "integration-tests")]

use std::sync::Arc;

use p2p_overlay::{OverlayService, PeerEndpoint};
use sys::tempfile::tempdir;
use the_block::config::{ensure_overlay_sanity, OverlayBackend, OverlayConfig};
use the_block::gossip::{config::GossipConfig, relay::Relay};
use the_block::net::{self, discovery, uptime, OverlayAddress, OverlayPeerId};
use the_block::simple_db::SimpleDb;

struct OverlayRestore(Arc<dyn OverlayService<Peer = OverlayPeerId, Address = OverlayAddress>>);

impl Drop for OverlayRestore {
    fn drop(&mut self) {
        net::install_overlay(self.0.clone());
    }
}

fn seeded_peer_id(seed: u8) -> discovery::PeerId {
    net::overlay_peer_from_bytes(&[seed; 32]).expect("overlay peer")
}

#[test]
fn relay_status_emits_base58_peers() {
    let mut cfg = GossipConfig::default();
    let dir = tempdir().unwrap();
    cfg.shard_store_path = dir.path().join("shards").to_string_lossy().into_owned();
    let relay = Relay::with_engine_factory(cfg, |name, path| SimpleDb::open_named(name, path));
    let peer_bytes = [7u8; 32];
    let peer = net::overlay_peer_from_bytes(&peer_bytes).expect("peer");
    relay.register_peer(9, peer.clone());

    let status = relay.status();
    let shard_entry = status
        .shard_affinity
        .into_iter()
        .find(|entry| entry.shard == 9)
        .expect("shard entry present");
    assert_eq!(shard_entry.peers, vec![net::overlay_peer_to_base58(&peer)],);
}

#[test]
fn relay_broadcast_records_selected_peers() {
    let mut cfg = GossipConfig::default();
    let dir = tempdir().unwrap();
    cfg.shard_store_path = dir.path().join("status").to_string_lossy().into_owned();
    let relay = Relay::with_engine_factory(cfg, |name, path| SimpleDb::open_named(name, path));
    let peer = net::overlay_peer_from_bytes(&[5u8; 32]).expect("overlay peer");
    let addr: std::net::SocketAddr = "127.0.0.1:9456".parse().unwrap();
    the_block::net::peer::inject_addr_mapping_for_tests(addr, peer.clone());

    let sk = crypto_suite::signatures::ed25519::SigningKey::from_bytes(&[4u8; 32]);
    let msg = the_block::net::Message::new(the_block::net::Payload::Hello(vec![]), &sk)
        .expect("sign hello");
    relay.broadcast_with(
        &msg,
        &[(addr, the_block::net::Transport::Tcp, None)],
        |_, _| {},
    );

    let status = relay.status();
    let selected = status
        .fanout
        .selected_peers
        .expect("selected peers present");
    assert_eq!(selected, vec![net::overlay_peer_to_base58(&peer)]);
}

#[test]
fn shard_affinity_emits_sorted_peers_per_shard() {
    let mut cfg = GossipConfig::default();
    let dir = tempdir().unwrap();
    cfg.shard_store_path = dir.path().join("sorted").to_string_lossy().into_owned();
    let relay = Relay::with_engine_factory(cfg, |name, path| SimpleDb::open_named(name, path));
    let relay = Arc::new(relay);
    let _guard = net::scoped_gossip_relay(Arc::clone(&relay));

    let peers = vec![
        net::overlay_peer_from_bytes(&[3u8; 32]).expect("peer"),
        net::overlay_peer_from_bytes(&[1u8; 32]).expect("peer"),
        net::overlay_peer_from_bytes(&[2u8; 32]).expect("peer"),
    ];

    for peer in &peers {
        net::register_shard_peer(7, peer.clone());
    }

    let status = net::gossip_status().expect("relay status available");
    let shard_entry = status
        .shard_affinity
        .into_iter()
        .find(|entry| entry.shard == 7)
        .expect("shard entry present");

    let mut expected = peers
        .iter()
        .map(|peer| net::overlay_peer_to_base58(peer))
        .collect::<Vec<_>>();
    expected.sort();

    assert_eq!(shard_entry.peers, expected);
    drop(relay);
}

#[test]
fn overlay_swap_end_to_end() {
    let previous = net::overlay_service();
    let _restore = OverlayRestore(previous);

    let dir = tempdir().unwrap();
    let path = dir.path().join("inhouse_peers.json");
    let inhouse_cfg = OverlayConfig {
        backend: OverlayBackend::Inhouse,
        peer_db_path: path.to_string_lossy().into_owned(),
    };
    net::configure_overlay(&inhouse_cfg);
    ensure_overlay_sanity(&inhouse_cfg).expect("in-house overlay sanity");

    let local = seeded_peer_id(11);
    let mut inhouse_discovery = discovery::new(local.clone());
    let other = seeded_peer_id(12);
    let addr = PeerEndpoint::new("127.0.0.1:9100".parse().unwrap());
    inhouse_discovery.add_peer(other.clone(), addr.clone());
    inhouse_discovery.persist();
    uptime::note_seen(other.clone());

    let status = net::overlay_status();
    assert_eq!(status.backend, "inhouse");
    assert_eq!(status.persisted_peers, 1);
    assert_eq!(status.active_peers, 1);
    assert_eq!(
        status.database_path.as_deref(),
        Some(inhouse_cfg.peer_db_path.as_str()),
    );

    let mut stub_cfg = inhouse_cfg.clone();
    stub_cfg.backend = OverlayBackend::Stub;
    net::configure_overlay(&stub_cfg);
    ensure_overlay_sanity(&stub_cfg).expect("stub overlay sanity");

    let mut stub_discovery = discovery::new(seeded_peer_id(13));
    let stub_peer = seeded_peer_id(14);
    stub_discovery.add_peer(stub_peer.clone(), addr);
    stub_discovery.persist();
    uptime::note_seen(stub_peer);

    let status = net::overlay_status();
    assert_eq!(status.backend, "stub");
    assert_eq!(status.persisted_peers, 1);
    assert_eq!(status.active_peers, 1);
    assert!(status.database_path.is_none());
}
