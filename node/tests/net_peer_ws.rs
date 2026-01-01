#![cfg(feature = "integration-tests")]
use std::time::Duration;
use the_block::net::peer::{broadcast_metrics, subscribe_peer_metrics, PeerMetrics};

fn peer_label(pk: &[u8; 32]) -> String {
    the_block::net::overlay_peer_from_bytes(pk)
        .map(|p| the_block::net::overlay_peer_to_base58(&p))
        .unwrap_or_else(|_| crypto_suite::hex::encode(pk))
}

#[test]
fn broadcast_updates() {
    runtime::block_on(async {
        let mut rx = subscribe_peer_metrics();
        let pk = [42u8; 32];
        let pm = PeerMetrics {
            requests: 1,
            ..Default::default()
        };
        broadcast_metrics(&pk, &pm);
        let snap = the_block::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("recv")
            .expect("snapshot");
        assert_eq!(snap.peer_id, peer_label(&pk));
        assert_eq!(snap.metrics.requests, 1);
    });
}
