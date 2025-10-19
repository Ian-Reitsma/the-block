#![cfg(feature = "integration-tests")]
use the_block::net::{overlay_peer_from_bytes, uptime};

#[test]
fn rebate_claim_once_per_epoch() {
    let peer = overlay_peer_from_bytes(&[7u8; 32]).expect("peer id");
    uptime::note_seen(peer.clone());
    assert_eq!(uptime::claim(peer.clone(), 0, 1, 10), Some(10));
    // second claim in same epoch rejected
    assert_eq!(uptime::claim(peer.clone(), 0, 1, 10), None);
    // next epoch allowed
    assert_eq!(uptime::claim(peer.clone(), 0, 2, 5), Some(5));
}
