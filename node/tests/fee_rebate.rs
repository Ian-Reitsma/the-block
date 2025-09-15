use the_block::net::uptime;
use libp2p::PeerId;

#[test]
fn rebate_claim_once_per_epoch() {
    let peer = PeerId::random();
    uptime::note_seen(peer.clone());
    assert_eq!(uptime::claim(peer.clone(), 0, 1, 10), Some(10));
    // second claim in same epoch rejected
    assert_eq!(uptime::claim(peer.clone(), 0, 1, 10), None);
    // next epoch allowed
    assert_eq!(uptime::claim(peer.clone(), 0, 2, 5), Some(5));
}
