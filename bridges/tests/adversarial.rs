use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    relayer::RelayerSet,
    Bridge, BridgeConfig, RelayerBundle, RelayerProof,
};
use tempfile::tempdir;

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
    let tmp = tempdir().expect("tempdir");
    let mut cfg = BridgeConfig::default();
    cfg.headers_dir = tmp.path().join("headers").to_str().unwrap().to_string();
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
    assert!(bridge
        .pending_withdrawals()
        .iter()
        .any(|(k, v)| *k == commitment && v.challenged));
    assert!(!bridge.finalize_withdrawal(commitment));
    assert_eq!(relayers.status("r1").unwrap().stake, 9);
    assert_eq!(relayers.status("r2").unwrap().stake, 9);
}

#[test]
fn malformed_proof_triggers_slash() {
    let tmp = tempdir().expect("tempdir");
    let mut cfg = BridgeConfig::default();
    cfg.headers_dir = tmp.path().join("headers").to_str().unwrap().to_string();
    let mut bridge = Bridge::new(cfg);
    let header = sample_header();
    let proof = sample_proof();
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 5);
    relayers.stake("r2", 5);
    let mut bundle = sample_bundle("alice", 5);
    // Corrupt the second relayer commitment to simulate a bad signature bundle.
    bundle.proofs[1].commitment = [1u8; 32];
    assert!(!bridge.deposit_with_relayer(
        &mut relayers,
        "r1",
        "alice",
        5,
        &header,
        &proof,
        &bundle
    ));
    let status = relayers.status("r1").unwrap();
    assert_eq!(status.slashes, 1);
    assert!(status.stake < 5);
}

#[test]
fn finalize_respects_challenge_window() {
    let tmp = tempdir().expect("tempdir");
    let mut cfg = BridgeConfig::default();
    cfg.headers_dir = tmp.path().join("headers").to_str().unwrap().to_string();
    let mut bridge = Bridge::new(cfg);
    let header = sample_header();
    let proof = sample_proof();
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 10);
    relayers.stake("r2", 10);
    let bundle = sample_bundle("alice", 4);
    assert!(bridge.deposit_with_relayer(&mut relayers, "r1", "alice", 4, &header, &proof, &bundle));
    assert!(bridge.unlock_with_relayer(&mut relayers, "r1", "alice", 4, &bundle));
    let commitment = bundle.aggregate_commitment("alice", 4);
    // Cannot finalize immediately while the challenge window is active.
    assert!(!bridge.finalize_withdrawal(commitment));
    // Retroactively move the initiation time into the past and attempt again.
    if let Some(pending) = bridge.pending_withdrawals.get_mut(&commitment) {
        pending.initiated_at = pending
            .initiated_at
            .saturating_sub(bridge.cfg.challenge_period_secs + 2);
    }
    assert!(bridge.finalize_withdrawal(commitment));
    assert!(bridge.pending_withdrawals().is_empty());
}
