use blake3;
use dex::escrow::{verify_proof, Escrow};

#[test]
fn full_and_partial_and_proofs() {
    let mut esc = Escrow::default();
    let id = esc.lock("alice".into(), "bob".into(), 100);
    // First partial release
    let proof1 = esc.release(id, 40).unwrap();
    let leaf1: [u8; 32] = *blake3::hash(&40u64.to_le_bytes()).as_bytes();
    assert!(verify_proof(proof1.leaf, 0, &proof1.path, leaf1));
    // Second release finalises escrow
    let proof2 = esc.release(id, 60).unwrap();
    let leaf2: [u8; 32] = *blake3::hash(&60u64.to_le_bytes()).as_bytes();
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(&leaf1);
    buf[32..].copy_from_slice(&leaf2);
    let root2: [u8; 32] = *blake3::hash(&buf).as_bytes();
    assert!(verify_proof(proof2.leaf, 1, &proof2.path, root2));
    assert!(esc.status(id).is_none());
}

#[test]
fn cancel_releases() {
    let mut esc = Escrow::default();
    let id = esc.lock("a".into(), "b".into(), 50);
    assert!(esc.cancel(id));
    assert!(esc.status(id).is_none());
}
