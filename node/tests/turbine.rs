use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use ed25519_dalek::SigningKey;
use the_block::net::{turbine, Message, Payload};
use the_block::p2p::handshake::Transport;

#[test]
fn turbine_fanout_reaches_all() {
    let sk = SigningKey::from_bytes(&[3u8; 32]);
    let msg = Message::new(Payload::Hello(vec![]), &sk);
    let peers: Vec<(SocketAddr, Transport, Option<Vec<u8>>)> = (0..31)
        .map(|i| {
            (
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1000 + i),
                Transport::Tcp,
                None,
            )
        })
        .collect();
    let mut sent = Vec::new();
    turbine::broadcast_with(&msg, &peers, |(addr, _, _), _| sent.push(addr));
    assert_eq!(sent.len(), peers.len());
    for (p, _, _) in peers {
        assert!(sent.contains(&p));
    }
}
