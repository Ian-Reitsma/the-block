#[path = "../peer_id/mod.rs"]
mod peer_id;

#[test]
fn harness_accepts_known_good_payload() {
    let peer = p2p_overlay::InhousePeerId::new([0x42; 32]);
    let encoded = peer.to_base58();
    peer_id::run(encoded.as_bytes());
}

#[test]
fn harness_ignores_binary_garbage() {
    peer_id::run(&[0u8, 0xff, 0x7f]);
}
