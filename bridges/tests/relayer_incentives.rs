use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    relayer::RelayerSet,
    Bridge, RelayerProof,
};

fn header() -> PowHeader {
    let merkle_root: [u8; 32] = [0u8; 32];
    let mut h = PowHeader {
        chain_id: "ext".into(),
        height: 1,
        merkle_root,
        signature: [0u8; 32],
        nonce: 0,
        target: u64::MAX,
    };
    let hdr = Header {
        chain_id: h.chain_id.clone(),
        height: h.height,
        merkle_root: h.merkle_root,
        signature: [0u8; 32],
    };
    h.signature = header_hash(&hdr);
    h
}

fn proof() -> Proof {
    Proof {
        leaf: [0u8; 32],
        path: vec![],
    }
}

#[test]
fn slashes_invalid_relayer() {
    let mut b = Bridge::default();
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 10);
    let hdr = header();
    let pf = proof();
    let rp = RelayerProof::new("r1", "alice", 5);
    // corrupted commitment
    let bad = RelayerProof {
        relayer: "r1".into(),
        commitment: [0u8; 32],
    };
    assert!(b.deposit_with_relayer(&mut relayers, "r1", "alice", 5, &hdr, &pf, &rp));
    assert_eq!(relayers.status("r1").unwrap().stake, 10);
    assert!(!b.deposit_with_relayer(&mut relayers, "r1", "alice", 5, &hdr, &pf, &bad));
    assert_eq!(relayers.status("r1").unwrap().stake, 9);
}
