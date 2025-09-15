#![allow(clippy::unwrap_used)]
use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    relayer::RelayerSet,
    Bridge, RelayerBundle, RelayerProof,
};

#[test]
fn bulk_deposits() {
    let mut bridge = Bridge::default();
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 1000);
    relayers.stake("r2", 1000);
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
    let bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 1),
        RelayerProof::new("r2", "alice", 1),
    ]);
    for _ in 0..100 {
        assert!(bridge.deposit_with_relayer(
            &mut relayers,
            "r1",
            "alice",
            1,
            &hdr,
            &proof,
            &bundle
        ));
    }
    assert_eq!(bridge.locked("alice"), 100);
}
