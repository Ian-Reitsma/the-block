use bridges::light_client::{header_hash, verify, Header, Proof};
use bridges::{
    header::{verify_pow, PowHeader},
    relayer::RelayerSet,
    Bridge, BridgeConfig, RelayerBundle, RelayerProof,
};
use crypto_suite::hashing::blake3::Hasher;
use sys::tempfile;

#[cfg(feature = "telemetry")]
use bridges::{PROOF_VERIFY_FAILURE_TOTAL, PROOF_VERIFY_SUCCESS_TOTAL};

fn sample_proof_valid() -> Proof {
    Proof {
        leaf: crypto_suite::hex::decode_array::<32>(
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap(),
        path: vec![
            crypto_suite::hex::decode_array::<32>(
                "0101010101010101010101010101010101010101010101010101010101010101",
            )
            .unwrap(),
            crypto_suite::hex::decode_array::<32>(
                "0202020202020202020202020202020202020202020202020202020202020202",
            )
            .unwrap(),
        ],
    }
}

fn sample_merkle_root() -> [u8; 32] {
    let proof = sample_proof_valid();
    let mut acc = proof.leaf;
    for sibling in &proof.path {
        let mut hasher = Hasher::new();
        hasher.update(&acc);
        hasher.update(sibling);
        acc = *hasher.finalize().as_bytes();
    }
    acc
}

fn sample_header() -> PowHeader {
    let merkle_root = sample_merkle_root();
    let mut h = PowHeader {
        chain_id: "ext".to_string(),
        height: 1,
        merkle_root,
        signature: [0u8; 32],
        nonce: 0,
        target: u64::MAX,
    };
    let header = Header {
        chain_id: h.chain_id.clone(),
        height: h.height,
        merkle_root: h.merkle_root,
        signature: [0u8; 32],
    };
    h.signature = header_hash(&header);
    h
}

fn pow_to_header(pow: &PowHeader) -> Header {
    Header {
        chain_id: pow.chain_id.clone(),
        height: pow.height,
        merkle_root: pow.merkle_root,
        signature: pow.signature,
    }
}

#[test]
fn deposit_valid_proof() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = BridgeConfig {
        headers_dir: dir.path().to_str().unwrap().into(),
        ..BridgeConfig::default()
    };
    let mut bridge = Bridge::new(cfg);
    #[cfg(feature = "telemetry")]
    let _counter_guard = bridges::proof_counter_test_guard();
    let header = sample_header();
    let proof = sample_proof_valid();
    let lc_header = pow_to_header(&header);
    assert!(verify_pow(&header), "PoW header should be valid");
    assert!(verify(&lc_header, &proof), "merkle proof should verify");
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 100);
    relayers.stake("r2", 100);
    let bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 50),
        RelayerProof::new("r2", "alice", 50),
    ]);
    let (valid, invalid) = bundle.verify("alice", 50);
    assert_eq!(valid, 2, "bundle failed with {:?}", invalid);
    // Use delta-based testing to avoid race conditions with parallel tests
    #[cfg(feature = "telemetry")]
    let (success_before, failure_before) = (
        (*PROOF_VERIFY_SUCCESS_TOTAL).get(),
        (*PROOF_VERIFY_FAILURE_TOTAL).get(),
    );
    assert!(bridge.deposit_with_relayer(
        &mut relayers,
        "r1",
        "alice",
        50,
        &header,
        &proof,
        &bundle
    ));
    assert_eq!(bridge.locked("alice"), 50);
    #[cfg(feature = "telemetry")]
    {
        let success_delta = (*PROOF_VERIFY_SUCCESS_TOTAL).get() - success_before;
        let failure_delta = (*PROOF_VERIFY_FAILURE_TOTAL).get() - failure_before;
        assert_eq!(success_delta, 1, "expected 1 success increment");
        assert_eq!(failure_delta, 0, "expected 0 failure increments");
    }
}

#[test]
fn deposit_invalid_proof() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = BridgeConfig {
        headers_dir: dir.path().to_str().unwrap().into(),
        ..BridgeConfig::default()
    };
    let mut bridge = Bridge::new(cfg);
    #[cfg(feature = "telemetry")]
    let _counter_guard = bridges::proof_counter_test_guard();
    let header = sample_header();
    let mut bad = sample_proof_valid();
    bad.path[0][0] ^= 0xff;
    let lc_header = pow_to_header(&header);
    assert!(verify_pow(&header));
    assert!(verify(&lc_header, &sample_proof_valid()));
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 100);
    relayers.stake("r2", 100);
    let bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 50),
        RelayerProof::new("r2", "alice", 50),
    ]);
    // Use delta-based testing to avoid race conditions with parallel tests
    #[cfg(feature = "telemetry")]
    let (success_before, failure_before) = (
        (*PROOF_VERIFY_SUCCESS_TOTAL).get(),
        (*PROOF_VERIFY_FAILURE_TOTAL).get(),
    );
    assert!(!bridge.deposit_with_relayer(&mut relayers, "r1", "alice", 50, &header, &bad, &bundle));
    assert_eq!(bridge.locked("alice"), 0);
    #[cfg(feature = "telemetry")]
    {
        let success_delta = (*PROOF_VERIFY_SUCCESS_TOTAL).get() - success_before;
        let failure_delta = (*PROOF_VERIFY_FAILURE_TOTAL).get() - failure_before;
        assert_eq!(success_delta, 0, "expected 0 success increments");
        assert_eq!(failure_delta, 1, "expected 1 failure increment");
    }
}

#[test]
fn deposit_replay_fails() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = BridgeConfig {
        headers_dir: dir.path().to_str().unwrap().into(),
        ..BridgeConfig::default()
    };
    let mut bridge = Bridge::new(cfg);
    #[cfg(feature = "telemetry")]
    let _counter_guard = bridges::proof_counter_test_guard();
    let header = sample_header();
    let proof = sample_proof_valid();
    let lc_header = pow_to_header(&header);
    assert!(verify_pow(&header));
    assert!(verify(&lc_header, &proof));
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 100);
    relayers.stake("r2", 100);
    let bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 50),
        RelayerProof::new("r2", "alice", 50),
    ]);
    // Use delta-based testing to avoid race conditions with parallel tests
    #[cfg(feature = "telemetry")]
    let (success_before, failure_before) = (
        (*PROOF_VERIFY_SUCCESS_TOTAL).get(),
        (*PROOF_VERIFY_FAILURE_TOTAL).get(),
    );
    assert!(bridge.deposit_with_relayer(
        &mut relayers,
        "r1",
        "alice",
        50,
        &header,
        &proof,
        &bundle
    ));
    assert!(!bridge.deposit_with_relayer(
        &mut relayers,
        "r1",
        "alice",
        50,
        &header,
        &proof,
        &bundle
    ));
    assert_eq!(bridge.locked("alice"), 50);
    #[cfg(feature = "telemetry")]
    {
        let success_delta = (*PROOF_VERIFY_SUCCESS_TOTAL).get() - success_before;
        let failure_delta = (*PROOF_VERIFY_FAILURE_TOTAL).get() - failure_before;
        assert_eq!(success_delta, 1, "expected 1 success increment");
        assert_eq!(failure_delta, 1, "expected 1 failure increment (replay)");
    }
}
