use ed25519_dalek::SigningKey;
use std::net::SocketAddr;
use std::time::Duration;
use the_block::gossip::relay::Relay;
use the_block::net::{Message, Payload};
use the_block::p2p::handshake::Transport;

#[test]
fn relay_dedup_and_fanout() {
    let relay = Relay::new(Duration::from_secs(2));
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let msg = Message::new(Payload::Hello(vec![]), &sk);
    assert!(relay.should_process(&msg));
    assert!(!relay.should_process(&msg));
    #[cfg(feature = "telemetry")]
    assert!(the_block::telemetry::GOSSIP_DUPLICATE_TOTAL.get() > 0);
    let peers: Vec<(SocketAddr, Transport, Option<Vec<u8>>)> = (0..25)
        .map(|i| {
            (
                format!("127.0.0.1:{}", 10000 + i).parse().unwrap(),
                Transport::Tcp,
                None,
            )
        })
        .collect();
    let msg2 = Message::new(Payload::Hello(vec![peers[0].0]), &sk);
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

#[test]
fn relay_mixed_transport_fanout() {
    std::env::set_var("TB_GOSSIP_FANOUT", "all");
    let relay = Relay::default();
    let sk = SigningKey::from_bytes(&[1u8; 32]);
    let msg = Message::new(Payload::Hello(vec![]), &sk);
    let peers = vec![
        ("127.0.0.1:10000".parse().unwrap(), Transport::Tcp, None),
        (
            "127.0.0.1:10001".parse().unwrap(),
            Transport::Quic,
            Some(vec![1, 2, 3]),
        ),
    ];
    let mut seen: Vec<(SocketAddr, Transport)> = Vec::new();
    relay.broadcast_with(&msg, &peers, |(addr, t, _), _| seen.push((addr, t)));
    assert_eq!(seen.len(), 2);
    assert!(seen
        .iter()
        .any(|(a, t)| (*a, *t) == (peers[0].0, peers[0].1)));
    assert!(seen
        .iter()
        .any(|(a, t)| (*a, *t) == (peers[1].0, peers[1].1)));
    std::env::remove_var("TB_GOSSIP_FANOUT");
}
