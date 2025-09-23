#![cfg(feature = "integration-tests")]
use hex;
use std::time::Duration;
use the_block::net::peer::{broadcast_metrics, subscribe_peer_metrics, PeerMetrics};

#[tokio::test]
async fn broadcast_updates() {
    let mut rx = subscribe_peer_metrics();
    let pk = [42u8; 32];
    let mut pm = PeerMetrics::default();
    pm.requests = 1;
    broadcast_metrics(&pk, &pm);
    let snap = the_block::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("recv")
        .expect("snapshot");
    assert_eq!(snap.peer_id, hex::encode(pk));
    assert_eq!(snap.metrics.requests, 1);
}
