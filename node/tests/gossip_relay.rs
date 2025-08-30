use ed25519_dalek::SigningKey;
use std::net::SocketAddr;
use std::time::Duration;
use the_block::gossip::relay::Relay;
use the_block::net::{Message, Payload};

#[test]
fn relay_dedup_and_fanout() {
    let relay = Relay::new(Duration::from_secs(2));
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let msg = Message::new(Payload::Hello(vec![]), &sk);
    assert!(relay.should_process(&msg));
    assert!(!relay.should_process(&msg));
    #[cfg(feature = "telemetry")]
    assert!(the_block::telemetry::GOSSIP_DUPLICATE_TOTAL.get() > 0);
    let peers: Vec<SocketAddr> = (0..25)
        .map(|i| format!("127.0.0.1:{}", 10000 + i).parse().unwrap())
        .collect();
    let msg2 = Message::new(Payload::Hello(vec![peers[0]]), &sk);
    let expected = ((peers.len() as f64).sqrt().ceil() as usize).min(16);
    let mut delivered = 0usize;
    let mut count = 0usize;
    let loss = (expected as f64 * 0.15).ceil() as usize;
    relay.broadcast_with(&msg2, &peers, |_, _| {
        if count >= loss {
            delivered += 1;
        }
        count += 1;
    });
    assert_eq!(count, expected);
    assert!(delivered >= expected - loss);
}
