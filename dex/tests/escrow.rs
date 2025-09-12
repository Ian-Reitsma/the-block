use blake3;
use dex::escrow::{verify_proof, Escrow};

#[test]
fn full_and_partial_and_proofs() {
    let mut esc = Escrow::default();
    let id = esc.lock("alice".into(), "bob".into(), 100);
    // First partial release
    let proof1 = esc.release(id, 40).unwrap();
    let leaf1: [u8; 32] = *blake3::hash(&40u64.to_le_bytes()).as_bytes();
    assert!(verify_proof(proof1.leaf, 0, &proof1.path, leaf1, proof1.algo));
    // Second release finalises escrow
    let proof2 = esc.release(id, 60).unwrap();
    let leaf2: [u8; 32] = *blake3::hash(&60u64.to_le_bytes()).as_bytes();
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(&leaf1);
    buf[32..].copy_from_slice(&leaf2);
    let root2: [u8; 32] = *blake3::hash(&buf).as_bytes();
    assert!(verify_proof(proof2.leaf, 1, &proof2.path, root2, proof2.algo));
    assert!(esc.status(id).is_none());
}

#[test]
fn multi_fill_trades() {
    let mut esc = Escrow::default();
    let id = esc.lock("alice".into(), "bob".into(), 100);
    assert!(esc.release(id, 20).is_some());
    assert!(esc.release(id, 30).is_some());
    let proof = esc.release(id, 50).unwrap();
    let l0: [u8; 32] = *blake3::hash(&20u64.to_le_bytes()).as_bytes();
    let l1: [u8; 32] = *blake3::hash(&30u64.to_le_bytes()).as_bytes();
    let l2: [u8; 32] = *blake3::hash(&50u64.to_le_bytes()).as_bytes();
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(&l0);
    buf[32..].copy_from_slice(&l1);
    let h01: [u8; 32] = *blake3::hash(&buf).as_bytes();
    let mut buf2 = [0u8; 64];
    buf2[..32].copy_from_slice(&l2);
    buf2[32..].copy_from_slice(&l2);
    let h22: [u8; 32] = *blake3::hash(&buf2).as_bytes();
    let mut buf3 = [0u8; 64];
    buf3[..32].copy_from_slice(&h01);
    buf3[32..].copy_from_slice(&h22);
    let root: [u8; 32] = *blake3::hash(&buf3).as_bytes();
    assert!(verify_proof(proof.leaf, 2, &proof.path, root, proof.algo));
    assert!(esc.status(id).is_none());
}

#[test]
fn premature_withdrawal_attempt() {
    let mut esc = Escrow::default();
    let id = esc.lock("alice".into(), "bob".into(), 30);
    assert!(esc.release(id, 40).is_none());
    assert!(esc.status(id).is_some());
}

#[test]
fn cancel_releases() {
    let mut esc = Escrow::default();
    let id = esc.lock("a".into(), "b".into(), 50);
    assert!(esc.cancel(id));
    assert!(esc.status(id).is_none());
}
