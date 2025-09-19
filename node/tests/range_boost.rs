#![cfg(feature = "integration-tests")]
use the_block::range_boost::{HopProof, RangeBoost};

#[test]
fn bundle_queue_works() {
    let mut rb = RangeBoost::new();
    rb.enqueue(vec![0u8; 4]);
    rb.record_proof(
        0,
        HopProof {
            relay: "loopback".into(),
        },
    );
    let b = rb.dequeue().unwrap();
    assert_eq!(b.payload.len(), 4);
    assert_eq!(b.proofs[0].relay, "loopback");
}

#[test]
fn parse_packet() {
    let data = b"unix:/tmp/sock,42";
    let peer = the_block::range_boost::parse_discovery_packet(data).unwrap();
    assert_eq!(peer.addr, "unix:/tmp/sock");
    assert_eq!(peer.latency_ms, 42);
}
