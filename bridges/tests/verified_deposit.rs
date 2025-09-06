use bridges::light_client::{Header, Proof};
use bridges::Bridge;

#[cfg(feature = "telemetry")]
use bridges::{PROOF_VERIFY_FAILURE_TOTAL, PROOF_VERIFY_SUCCESS_TOTAL};

fn sample_header() -> Header {
    Header {
        chain_id: "ext".to_string(),
        height: 1,
        merkle_root: hex::decode(
            "bb5a8ac31a71fd564acd5f4614a88ebaf771108e2f40838219f6dbec309ef23d",
        )
        .unwrap()
        .try_into()
        .unwrap(),
        signature: hex::decode("0b16e7c6c7589326e1b89904a9348397061379432c4204cad5fe221178dd983e")
            .unwrap()
            .try_into()
            .unwrap(),
    }
}

fn sample_proof_valid() -> Proof {
    Proof {
        leaf: hex::decode("0000000000000000000000000000000000000000000000000000000000000000")
            .unwrap()
            .try_into()
            .unwrap(),
        path: vec![
            hex::decode("0101010101010101010101010101010101010101010101010101010101010101")
                .unwrap()
                .try_into()
                .unwrap(),
            hex::decode("0202020202020202020202020202020202020202020202020202020202020202")
                .unwrap()
                .try_into()
                .unwrap(),
        ],
    }
}

#[test]
fn deposit_valid_proof() {
    let mut bridge = Bridge::default();
    let header = sample_header();
    let proof = sample_proof_valid();
    #[cfg(feature = "telemetry")]
    {
        PROOF_VERIFY_SUCCESS_TOTAL.reset();
        PROOF_VERIFY_FAILURE_TOTAL.reset();
    }
    assert!(bridge.deposit_verified("alice", 50, &header, &proof));
    assert_eq!(bridge.locked("alice"), 50);
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(PROOF_VERIFY_SUCCESS_TOTAL.get(), 1);
        assert_eq!(PROOF_VERIFY_FAILURE_TOTAL.get(), 0);
    }
}

#[test]
fn deposit_invalid_proof() {
    let mut bridge = Bridge::default();
    let header = sample_header();
    let mut bad = sample_proof_valid();
    bad.path[0][0] ^= 0xff;
    #[cfg(feature = "telemetry")]
    {
        PROOF_VERIFY_SUCCESS_TOTAL.reset();
        PROOF_VERIFY_FAILURE_TOTAL.reset();
    }
    assert!(!bridge.deposit_verified("alice", 50, &header, &bad));
    assert_eq!(bridge.locked("alice"), 0);
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(PROOF_VERIFY_SUCCESS_TOTAL.get(), 0);
        assert_eq!(PROOF_VERIFY_FAILURE_TOTAL.get(), 1);
    }
}

#[test]
fn deposit_replay_fails() {
    let mut bridge = Bridge::default();
    let header = sample_header();
    let proof = sample_proof_valid();
    #[cfg(feature = "telemetry")]
    {
        PROOF_VERIFY_SUCCESS_TOTAL.reset();
        PROOF_VERIFY_FAILURE_TOTAL.reset();
    }
    assert!(bridge.deposit_verified("alice", 50, &header, &proof));
    assert!(!bridge.deposit_verified("alice", 50, &header, &proof));
    assert_eq!(bridge.locked("alice"), 50);
    #[cfg(feature = "telemetry")]
    {
        assert_eq!(PROOF_VERIFY_SUCCESS_TOTAL.get(), 1);
        assert_eq!(PROOF_VERIFY_FAILURE_TOTAL.get(), 1);
    }
}
