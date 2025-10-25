#![allow(clippy::unwrap_used, clippy::expect_used)]

use bridge_types::BridgeIncentiveParameters;
use bridges::{
    header::PowHeader,
    light_client::{header_hash, Header, Proof},
    RelayerBundle, RelayerProof,
};
use concurrency::Lazy;
use governance_spec::{codec::encode_binary, ApprovedRelease};
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
        let lock = GOV_DB_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
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
        requires_settlement_proof: false,
        settlement_chain: None,
    };
    bridge
        .set_channel_config("native", config)
        .expect("configure channel");
}

fn approve_release(gov_path: &Path, asset: &str, commitment: &[u8; 32]) -> String {
    let release_key = format!("bridge:{asset}:{}", crypto_suite::hex::encode(commitment));
    let approved = ApprovedRelease {
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
        encode_binary(&approved).expect("serialize"),
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
    let min_bond = BridgeIncentiveParameters::DEFAULT_MIN_BOND;
    bridge.bond_relayer("r1", min_bond).unwrap();
    bridge.bond_relayer("r2", min_bond).unwrap();

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

    let (asset_id, info) = bridge
        .relayer_status("r1", Some("native"))
        .expect("relayer status");
    assert_eq!(asset_id, "native");
    let bond_after = info.bond;
    assert!(bond_after < min_bond);
    assert!(!bridge.slash_log().is_empty());
    assert!(matches!(
        bridge.finalize_withdrawal("native", commitment),
        Err(BridgeError::AlreadyChallenged)
    ));
}

#[test]
fn bridge_pending_dispute_persists_across_restart() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
    configure_native_channel(&mut bridge, &headers_dir, 120);
    let min_bond = BridgeIncentiveParameters::DEFAULT_MIN_BOND;
    bridge.bond_relayer("r1", min_bond).unwrap();
    bridge.bond_relayer("r2", min_bond).unwrap();

    let header = sample_header();
    let proof = sample_proof();
    let bundle = sample_bundle("alice", 12);
    bridge
        .deposit("native", "r1", "alice", 12, &header, &proof, &bundle)
        .expect("deposit");
    let commitment = bundle.aggregate_commitment("alice", 12);
    approve_release(&gov_path, "native", &commitment);
    bridge
        .request_withdrawal("native", "r1", "alice", 12, &bundle)
        .expect("request withdrawal");

    let challenge = bridge
        .challenge_withdrawal("native", commitment, "auditor")
        .expect("challenge withdrawal");
    assert_eq!(challenge.commitment, commitment);

    let pending_before = bridge.pending_withdrawals(Some("native"));
    assert!(pending_before
        .iter()
        .any(|entry| entry.commitment == commitment && entry.challenged));

    drop(bridge);

    let bridge_path_str = bridge_path.to_str().expect("bridge path str");
    let reopened = Bridge::open(bridge_path_str);
    let pending_after = reopened.pending_withdrawals(Some("native"));
    let pending_entry = pending_after
        .iter()
        .find(|entry| entry.commitment == commitment)
        .expect("pending withdrawal after restart");
    assert!(pending_entry.challenged);

    let (disputes_after_restart, _) = reopened.dispute_audit(Some("native"), None, 256);
    let dispute = disputes_after_restart
        .iter()
        .find(|entry| entry.commitment == commitment)
        .expect("dispute entry after restart");
    assert!(dispute.challenged);
    assert_eq!(dispute.challenger.as_deref(), Some("auditor"));
    assert!(dispute.challenged_at.is_some());
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
    let min_bond = BridgeIncentiveParameters::DEFAULT_MIN_BOND;
    bridge.bond_relayer("r1", min_bond).unwrap();
    bridge.bond_relayer("r2", min_bond).unwrap();

    let header = sample_header();
    let proof = sample_proof();
    let invalid_bundle = RelayerBundle::new(vec![RelayerProof::new("r1", "alice", 5)]);
    let err = bridge
        .deposit("native", "r1", "alice", 5, &header, &proof, &invalid_bundle)
        .unwrap_err();
    assert!(matches!(err, BridgeError::InvalidProof));
    let (asset, info) = bridge
        .relayer_status("r1", Some("native"))
        .expect("relayer status");
    assert_eq!(asset, "native");
    assert!(info.slashes >= 1);
    assert!(info.bond < min_bond);
    let deficit = min_bond.saturating_sub(info.bond);
    if deficit > 0 {
        bridge.bond_relayer("r1", deficit).unwrap();
    }
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

    let (_, info_after) = bridge
        .relayer_status("r1", Some("native"))
        .expect("relayer status");
    assert!(info_after.slashes >= 2);
    let failure_slash = BridgeIncentiveParameters::DEFAULT_FAILURE_SLASH;
    assert!(info_after.bond <= min_bond.saturating_sub(failure_slash));
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
    let min_bond = BridgeIncentiveParameters::DEFAULT_MIN_BOND;
    bridge.bond_relayer("r1", min_bond).unwrap();
    bridge.bond_relayer("r2", min_bond).unwrap();

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
    assert_eq!(pending[0].asset, "native");
    assert_eq!(
        crypto_suite::hex::encode(pending[0].commitment),
        expected_commitment
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
    let min_bond = BridgeIncentiveParameters::DEFAULT_MIN_BOND;
    bridge.bond_relayer("r1", min_bond).unwrap();
    bridge.bond_relayer("r2", min_bond).unwrap();

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
