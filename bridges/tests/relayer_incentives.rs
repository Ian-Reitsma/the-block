use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    relayer::RelayerSet,
    Bridge, BridgeConfig, RelayerBundle, RelayerProof,
};
use sys::tempfile;

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
    let dir = tempfile::tempdir().unwrap();
    let cfg = BridgeConfig {
        headers_dir: dir.path().to_str().unwrap().into(),
        ..BridgeConfig::default()
    };
    let mut b = Bridge::new(cfg);
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 10);
    relayers.stake("r2", 10);
    let hdr = header();
    let pf = proof();
    let good_bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 5),
        RelayerProof::new("r2", "alice", 5),
    ]);
    // corrupted commitment on the second signer
    let bad_bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 5),
        RelayerProof {
            relayer: "r2".into(),
            commitment: [0u8; 32],
        },
    ]);
    assert!(b.deposit_with_relayer(&mut relayers, "r1", "alice", 5, &hdr, &pf, &good_bundle));
    assert_eq!(relayers.status("r1").unwrap().stake, 10);
    assert!(!b.deposit_with_relayer(&mut relayers, "r1", "alice", 5, &hdr, &pf, &bad_bundle));
    assert_eq!(relayers.status("r2").unwrap().stake, 9);
}
