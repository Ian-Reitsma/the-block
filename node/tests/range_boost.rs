use the_block::range_boost::{HopProof, RangeBoost};

#[test]
fn bundle_queue_works() {
    let mut rb = RangeBoost::new();
    rb.enqueue(vec![0u8; 4]);
    rb.record_proof(
        0,
        HopProof {
            relay: "loopback".into(),
            credits: 1,
        },
    );
    let b = rb.dequeue().unwrap();
    assert_eq!(b.payload.len(), 4);
    assert_eq!(b.proofs[0].relay, "loopback");
}
