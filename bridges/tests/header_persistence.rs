use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    relayer::RelayerSet,
    Bridge, BridgeConfig, RelayerBundle, RelayerProof,
};
use tempfile::tempdir;

fn sample() -> (PowHeader, Proof) {
    let merkle_root = [0u8; 32];
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
    let pf = Proof {
        leaf: [0u8; 32],
        path: vec![],
    };
    (h, pf)
}

#[test]
fn persists_and_loads_headers() {
    let dir = tempdir().unwrap();
    let cfg = BridgeConfig {
        headers_dir: dir.path().to_str().unwrap().into(),
        ..BridgeConfig::default()
    };
    let mut bridge = Bridge::new(cfg.clone());
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 10);
    relayers.stake("r2", 10);
    let (hdr, pf) = sample();
    let bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 5),
        RelayerProof::new("r2", "alice", 5),
    ]);
    assert!(bridge.deposit_with_relayer(&mut relayers, "r1", "alice", 5, &hdr, &pf, &bundle));
    drop(bridge);
    let mut bridge2 = Bridge::new(cfg);
    // second deposit with same header should fail due to persisted header
    assert!(!bridge2.deposit_with_relayer(&mut relayers, "r1", "alice", 5, &hdr, &pf, &bundle));
}
