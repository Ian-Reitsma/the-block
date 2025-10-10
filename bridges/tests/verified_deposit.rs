use bridges::light_client::{header_hash, Header, Proof};
use bridges::{
    header::PowHeader, relayer::RelayerSet, Bridge, BridgeConfig, RelayerBundle, RelayerProof,
};
use foundation_serialization::hex;
use sys::tempfile;

#[cfg(feature = "telemetry")]
use bridges::{PROOF_VERIFY_FAILURE_TOTAL, PROOF_VERIFY_SUCCESS_TOTAL};

fn sample_header() -> PowHeader {
    let merkle_root = crypto_suite::hex::decode_array::<32>(
        "bb5a8ac31a71fd564acd5f4614a88ebaf771108e2f40838219f6dbec309ef23d",
    )
    .unwrap();
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

#[test]
fn deposit_valid_proof() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = BridgeConfig {
        headers_dir: dir.path().to_str().unwrap().into(),
        ..BridgeConfig::default()
    };
    let mut bridge = Bridge::new(cfg);
    let header = sample_header();
    let proof = sample_proof_valid();
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 100);
    relayers.stake("r2", 100);
    let bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 50),
        RelayerProof::new("r2", "alice", 50),
    ]);
    #[cfg(feature = "telemetry")]
    {
        PROOF_VERIFY_SUCCESS_TOTAL.reset();
        PROOF_VERIFY_FAILURE_TOTAL.reset();
    }
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
        assert_eq!(PROOF_VERIFY_SUCCESS_TOTAL.get(), 1);
        assert_eq!(PROOF_VERIFY_FAILURE_TOTAL.get(), 0);
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
    let header = sample_header();
    let mut bad = sample_proof_valid();
    bad.path[0][0] ^= 0xff;
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 100);
    relayers.stake("r2", 100);
    let bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 50),
        RelayerProof::new("r2", "alice", 50),
    ]);
    #[cfg(feature = "telemetry")]
    {
        PROOF_VERIFY_SUCCESS_TOTAL.reset();
        PROOF_VERIFY_FAILURE_TOTAL.reset();
    }
    assert!(!bridge.deposit_with_relayer(&mut relayers, "r1", "alice", 50, &header, &bad, &bundle));
    assert_eq!(bridge.locked("alice"), 0);
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(PROOF_VERIFY_SUCCESS_TOTAL.get(), 0);
        assert_eq!(PROOF_VERIFY_FAILURE_TOTAL.get(), 1);
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
    let header = sample_header();
    let proof = sample_proof_valid();
    let mut relayers = RelayerSet::default();
    relayers.stake("r1", 100);
    relayers.stake("r2", 100);
    let bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 50),
        RelayerProof::new("r2", "alice", 50),
    ]);
    #[cfg(feature = "telemetry")]
    {
        PROOF_VERIFY_SUCCESS_TOTAL.reset();
        PROOF_VERIFY_FAILURE_TOTAL.reset();
    }
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
        assert_eq!(PROOF_VERIFY_SUCCESS_TOTAL.get(), 1);
        assert_eq!(PROOF_VERIFY_FAILURE_TOTAL.get(), 1);
    }
}
