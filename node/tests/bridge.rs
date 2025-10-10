#![allow(clippy::unwrap_used, clippy::expect_used)]

use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    RelayerBundle, RelayerProof,
};
use concurrency::Lazy;
use sled::Config;
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use sys::tempfile::tempdir;
use the_block::bridge::{Bridge, BridgeError, ChannelConfig};

static GOV_DB_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

struct GovEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<String>,
}

impl GovEnvGuard {
    fn set(path: &Path) -> Self {
        let lock = GOV_DB_MUTEX.lock().expect("gov env lock");
        let previous = std::env::var("TB_GOV_DB_PATH").ok();
        let value = path.to_str().expect("gov path str").to_string();
        std::env::set_var("TB_GOV_DB_PATH", &value);
        Self {
            _lock: lock,
            previous,
        }
    }
}

impl Drop for GovEnvGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.previous.take() {
            std::env::set_var("TB_GOV_DB_PATH", prev);
        } else {
            std::env::remove_var("TB_GOV_DB_PATH");
        }
    }
}

fn configure_native_channel(bridge: &mut Bridge, headers_dir: &Path, challenge_period_secs: u64) {
    let config = ChannelConfig {
        asset: "native".into(),
        confirm_depth: 1,
        fee_per_byte: 0,
        challenge_period_secs,
        relayer_quorum: 2,
        headers_dir: headers_dir.to_str().expect("headers dir").to_string(),
    };
    bridge
        .set_channel_config("native", config)
        .expect("configure channel");
}

fn approve_release(gov_path: &Path, asset: &str, commitment: &[u8; 32]) -> String {
    let release_key = format!("bridge:{asset}:{}", crypto_suite::hex::encode(commitment));
    let approved = the_block::governance::ApprovedRelease {
        build_hash: release_key.clone(),
        activated_epoch: 0,
        proposer: "tester".into(),
        signatures: Vec::new(),
        signature_threshold: 0,
        signer_set: Vec::new(),
        install_times: Vec::new(),
    };
    let db = Config::new()
        .path(gov_path)
        .temporary(false)
        .open()
        .expect("open gov db");
    let tree = db.open_tree("approved_releases").expect("tree");
    tree.insert(
        release_key.as_bytes(),
        bincode::serialize(&approved).expect("serialize"),
    )
    .expect("insert");
    tree.flush().expect("flush");
    release_key
}

fn sample_header() -> PowHeader {
    let mut h = PowHeader {
        chain_id: "native".into(),
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

fn sample_bundle(user: &str, amount: u64) -> RelayerBundle {
    RelayerBundle::new(vec![
        RelayerProof::new("r1", user, amount),
        RelayerProof::new("r2", user, amount),
    ])
}

#[test]
fn bridge_records_receipts_and_slashes() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
    configure_native_channel(&mut bridge, &headers_dir, 30);
    bridge.bond_relayer("r1", 5).unwrap();

    let header = sample_header();
    let proof = sample_proof();
    let bundle = sample_bundle("alice", 5);

    let receipt = bridge
        .deposit("native", "r1", "alice", 5, &header, &proof, &bundle)
        .expect("deposit");
    assert_eq!(receipt.nonce, 0);
    assert_eq!(bridge.deposit_history("native", None, 10).len(), 1);

    let commitment = bundle.aggregate_commitment("alice", 5);
    approve_release(&gov_path, "native", &commitment);

    let pending_commitment = bridge
        .request_withdrawal("native", "r1", "alice", 5, &bundle)
        .expect("withdraw request");
    assert_eq!(pending_commitment, commitment);

    let challenge = bridge
        .challenge_withdrawal("native", commitment, "auditor")
        .expect("challenge");
    assert_eq!(challenge.asset, "native");

    let relayer_status = bridge
        .relayer_status("r1", Some("native"))
        .expect("relayer status");
    let (_, _, _, bond_after) = relayer_status;
    assert!(bond_after < 5);
    assert!(!bridge.slash_log().is_empty());
    assert!(matches!(
        bridge.finalize_withdrawal("native", commitment),
        Err(BridgeError::AlreadyChallenged)
    ));
}

#[test]
fn bridge_slashes_on_invalid_proofs() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
    configure_native_channel(&mut bridge, &headers_dir, 30);
    bridge.bond_relayer("r1", 5).unwrap();

    let header = sample_header();
    let proof = sample_proof();
    let invalid_bundle = RelayerBundle::new(vec![RelayerProof::new("r1", "alice", 5)]);
    let err = bridge
        .deposit("native", "r1", "alice", 5, &header, &proof, &invalid_bundle)
        .unwrap_err();
    assert!(matches!(err, BridgeError::InvalidProof));
    let (asset, _stake, slashes, bond) = bridge
        .relayer_status("r1", Some("native"))
        .expect("relayer status");
    assert_eq!(asset, "native");
    assert!(slashes >= 1);
    assert!(bond <= 4);
    assert!(!bridge.slash_log().is_empty());

    let header = sample_header();
    let proof = sample_proof();
    let bundle = sample_bundle("alice", 5);
    let receipt = bridge
        .deposit("native", "r1", "alice", 5, &header, &proof, &bundle)
        .expect("deposit");
    assert_eq!(receipt.nonce, 0);

    let commitment = bundle.aggregate_commitment("alice", 5);
    approve_release(&gov_path, "native", &commitment);

    let withdraw_bundle = RelayerBundle::new(vec![
        RelayerProof::new("r1", "alice", 5),
        // Provide a malformed secondary proof so quorum fails while the commitment
        // remains authorized.
        RelayerProof::new("r2", "alice", 6),
    ]);
    let err = bridge
        .request_withdrawal("native", "r1", "alice", 5, &withdraw_bundle)
        .unwrap_err();
    assert!(matches!(err, BridgeError::InvalidProof));

    let (_, _stake_after, slashes_after, bond_after) = bridge
        .relayer_status("r1", Some("native"))
        .expect("relayer status");
    assert!(slashes_after >= 2);
    assert!(bond_after <= 3);
    assert!(bridge.slash_log().len() >= 2);
    assert!(bridge.pending_withdrawals(None).is_empty());
}

#[test]
fn bridge_rejects_replay_and_persists_state() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
    configure_native_channel(&mut bridge, &headers_dir, 1);
    bridge.bond_relayer("r1", 5).unwrap();

    let header = sample_header();
    let proof = sample_proof();
    let bundle = sample_bundle("alice", 5);

    let receipt = bridge
        .deposit("native", "r1", "alice", 5, &header, &proof, &bundle)
        .expect("deposit");
    assert_eq!(receipt.nonce, 0);

    let replay_err = bridge
        .deposit("native", "r1", "alice", 5, &header, &proof, &bundle)
        .unwrap_err();
    assert!(matches!(replay_err, BridgeError::Replay));

    let commitment = bundle.aggregate_commitment("alice", 5);
    approve_release(&gov_path, "native", &commitment);
    let pending_commitment = bridge
        .request_withdrawal("native", "r1", "alice", 5, &bundle)
        .expect("withdraw request");
    assert_eq!(pending_commitment, commitment);

    drop(bridge);

    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
    configure_native_channel(&mut bridge, &headers_dir, 1);

    let history = bridge.deposit_history("native", None, 10);
    assert_eq!(history.len(), 1);
    assert_eq!(bridge.locked_balance("native", "alice"), Some(0));

    let pending = bridge.pending_withdrawals(None);
    let expected_commitment = crypto_suite::hex::encode(commitment);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["asset"].as_str(), Some("native"));
    assert_eq!(
        pending[0]["commitment"].as_str(),
        Some(expected_commitment.as_str())
    );

    thread::sleep(Duration::from_secs(2));
    assert!(matches!(
        bridge.finalize_withdrawal("native", commitment),
        Ok(())
    ));
    assert!(bridge.pending_withdrawals(None).is_empty());
}

#[test]
fn bridge_respects_challenge_window() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
    configure_native_channel(&mut bridge, &headers_dir, 2);
    bridge.bond_relayer("r1", 5).unwrap();

    let header = sample_header();
    let proof = sample_proof();
    let bundle = sample_bundle("alice", 5);

    bridge
        .deposit("native", "r1", "alice", 5, &header, &proof, &bundle)
        .expect("deposit");

    let commitment = bundle.aggregate_commitment("alice", 5);
    approve_release(&gov_path, "native", &commitment);
    bridge
        .request_withdrawal("native", "r1", "alice", 5, &bundle)
        .expect("withdraw request");

    assert!(matches!(
        bridge.finalize_withdrawal("native", commitment),
        Err(BridgeError::ChallengeWindowOpen)
    ));

    thread::sleep(Duration::from_secs(3));
    assert!(matches!(
        bridge.finalize_withdrawal("native", commitment),
        Ok(())
    ));
}
