#![allow(clippy::unwrap_used)]
use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    relayer::RelayerSet,
    Bridge, RelayerProof,
};

#[test]
fn bulk_deposits() {
    let mut bridge = Bridge::default();
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 1000);
    let mut hdr = PowHeader {
        chain_id: "ext".into(),
        height: 1,
        merkle_root: [0u8; 32],
        signature: [0u8; 32],
        nonce: 0,
        target: u64::MAX,
    };
    let h = Header {
        chain_id: hdr.chain_id.clone(),
        height: hdr.height,
        merkle_root: hdr.merkle_root,
        signature: [0u8; 32],
    };
    hdr.signature = header_hash(&h);
    let proof = Proof {
        leaf: [0u8; 32],
        path: vec![],
    };
    for _ in 0..100 {
        let rp = RelayerProof::new("r1", "alice", 1);
        assert!(bridge.deposit_with_relayer(&mut relayers, "r1", "alice", 1, &hdr, &proof, &rp));
    }
    assert_eq!(bridge.locked("alice"), 100);
}
