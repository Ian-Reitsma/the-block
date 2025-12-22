#![cfg(feature = "integration-tests")]
use std::time::Duration;
use the_block::net::peer::{broadcast_metrics, subscribe_peer_metrics, PeerMetrics};

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
        assert_eq!(snap.peer_id, crypto_suite::hex::encode(pk));
        assert_eq!(snap.metrics.requests, 1);
    });
}
