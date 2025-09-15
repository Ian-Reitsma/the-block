use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    relayer::RelayerSet,
    Bridge, BridgeConfig, RelayerBundle, RelayerProof,
};

fn sample_bundle(user: &str, amount: u64) -> RelayerBundle {
    RelayerBundle::new(vec![
        RelayerProof::new("r1", user, amount),
        RelayerProof::new("r2", user, amount),
    ])
}

fn sample_header() -> PowHeader {
    let mut h = PowHeader {
        chain_id: "adv".into(),
        height: 1,
        merkle_root: [0u8; 32],
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

fn sample_proof() -> Proof {
    Proof {
        leaf: [0u8; 32],
        path: vec![],
    }
}

#[test]
fn challenge_reverts_pending_withdrawal() {
    let cfg = BridgeConfig::default();
    let mut bridge = Bridge::new(cfg);
    let header = sample_header();
    let proof = sample_proof();
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 10);
    relayers.stake("r2", 10);
    let bundle = sample_bundle("alice", 5);
    assert!(bridge.deposit_with_relayer(&mut relayers, "r1", "alice", 5, &header, &proof, &bundle));
    assert!(bridge.unlock_with_relayer(&mut relayers, "r1", "alice", 5, &bundle));
    let commitment = bundle.aggregate_commitment("alice", 5);
    assert!(bridge.challenge_withdrawal(&mut relayers, commitment));
    assert!(bridge.pending_withdrawals().iter().any(|(k, v)| *k == commitment && v.challenged));
    assert!(!bridge.finalize_withdrawal(commitment));
    assert_eq!(relayers.status("r1").unwrap().stake, 10);
    assert_eq!(relayers.status("r2").unwrap().stake, 9);
}
