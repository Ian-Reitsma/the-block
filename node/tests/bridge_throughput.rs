#![cfg(feature = "integration-tests")]
#![allow(clippy::unwrap_used)]
use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    relayer::RelayerSet,
    Bridge, BridgeConfig, RelayerBundle, RelayerProof,
};
use sys::tempfile::tempdir;

fn make_pow_header(height: u64) -> PowHeader {
    let mut hdr = PowHeader {
        chain_id: "ext".into(),
        height,
        merkle_root: [0u8; 32],
        signature: [0u8; 32],
        nonce: height,
        target: u64::MAX,
    };
    let header = Header {
        chain_id: hdr.chain_id.clone(),
        height: hdr.height,
        merkle_root: hdr.merkle_root,
        signature: [0u8; 32],
    };
    hdr.signature = header_hash(&header);
    hdr
}

#[test]
fn bulk_deposits() {
    let dir = tempdir().expect("create temp dir");
    let cfg = BridgeConfig {
        headers_dir: dir.path().to_string_lossy().into(),
        ..BridgeConfig::default()
    };
    let mut bridge = Bridge::new(cfg);
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 1000);
    relayers.stake("r2", 1000);
    let proof = Proof {
        leaf: [0u8; 32],
        path: vec![],
    };
    let bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 1),
        RelayerProof::new("r2", "alice", 1),
    ]);
    for height in 1..=100 {
        let hdr = make_pow_header(height);
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
