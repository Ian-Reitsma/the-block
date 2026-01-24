#![allow(clippy::unwrap_used, clippy::expect_used)]

use bridge_types::{
    settlement_proof_digest, BridgeIncentiveParameters, DutyStatus, ExternalSettlementProof,
};
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
use sys::tempfile::tempdir;
use the_block::bridge::{
    global_incentives, set_global_incentives, Bridge, BridgeError, ChannelConfig,
};
use the_block::governance::{GovStore, RewardClaimApproval};
#[cfg(feature = "test-telemetry")]
use the_block::telemetry::{
    BRIDGE_DISPUTE_OUTCOMES_TOTAL, BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL,
    BRIDGE_REWARD_CLAIMS_TOTAL, BRIDGE_SETTLEMENT_RESULTS_TOTAL, LABEL_REGISTRATION_ERR,
};

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

struct IncentiveGuard {
    previous: BridgeIncentiveParameters,
}

impl IncentiveGuard {
    fn set(params: BridgeIncentiveParameters) -> Self {
        let previous = global_incentives();
        set_global_incentives(params);
        Self { previous }
    }
}

impl Drop for IncentiveGuard {
    fn drop(&mut self) {
        set_global_incentives(self.previous.clone());
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

fn approve_release(gov_path: &Path, asset: &str, commitment: &[u8; 32]) {
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

fn sample_header_with_height(height: u64) -> PowHeader {
    let mut header = sample_header();
    header.height = height;
    let hdr = Header {
        chain_id: header.chain_id.clone(),
        height,
        merkle_root: header.merkle_root,
        signature: [0u8; 32],
    };
    header.signature = header_hash(&hdr);
    header
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
fn bridge_incentive_accounting_tracks_rewards_and_slashes() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
    configure_native_channel(&mut bridge, &headers_dir, 0);

    let params = BridgeIncentiveParameters {
        min_bond: 10,
        duty_reward: 25,
        failure_slash: 12,
        challenge_slash: 30,
        duty_window_secs: 120,
    };
    let _incentive_guard = IncentiveGuard::set(params.clone());

    bridge.bond_relayer("r1", 200).unwrap();
    bridge.bond_relayer("r2", 200).unwrap();

    let header = sample_header_with_height(2);
    let proof = sample_proof();
    let bundle = sample_bundle("alice", 50);

    let receipt = bridge
        .deposit("native", "r1", "alice", 50, &header, &proof, &bundle)
        .expect("deposit");
    assert_eq!(receipt.nonce, 0);

    let (asset, info_r1) = bridge
        .relayer_status("r1", Some("native"))
        .expect("relayer status");
    assert_eq!(asset, "native");
    assert_eq!(info_r1.rewards_earned, params.duty_reward);
    assert_eq!(info_r1.duties_completed, 1);
    assert_eq!(info_r1.pending_duties, 0);

    let commitment = bundle.aggregate_commitment("alice", 50);
    approve_release(&gov_path, "native", &commitment);
    let commitment = bridge
        .request_withdrawal("native", "r1", "alice", 50, &bundle)
        .expect("withdraw request");
    bridge
        .finalize_withdrawal("native", commitment)
        .expect("finalize");

    let (_, info_r1_after) = bridge
        .relayer_status("r1", Some("native"))
        .expect("relayer status");
    assert_eq!(info_r1_after.rewards_earned, params.duty_reward * 2);
    assert_eq!(info_r1_after.duties_completed, 2);
    let (_, info_r2_after) = bridge
        .relayer_status("r2", Some("native"))
        .expect("relayer status");
    assert_eq!(info_r2_after.rewards_earned, params.duty_reward);
    assert_eq!(info_r2_after.duties_assigned, 1);
    assert_eq!(info_r2_after.duties_completed, 1);

    let duty_records = bridge.duty_log(None, Some("native"), 16);
    assert!(duty_records
        .iter()
        .any(|record| record.relayer == "r1"
            && matches!(record.status, DutyStatus::Completed { .. })));
    assert!(duty_records
        .iter()
        .any(|record| record.relayer == "r2"
            && matches!(record.status, DutyStatus::Completed { .. })));

    // Prepare another withdrawal and challenge it to trigger slashing across the bundle.
    let header = sample_header_with_height(3);
    let proof = sample_proof();
    let bundle = sample_bundle("alice", 25);
    bridge
        .deposit("native", "r1", "alice", 25, &header, &proof, &bundle)
        .expect("second deposit");
    let commitment = bundle.aggregate_commitment("alice", 25);
    approve_release(&gov_path, "native", &commitment);
    let commitment = bridge
        .request_withdrawal("native", "r1", "alice", 25, &bundle)
        .expect("withdraw request");
    bridge
        .challenge_withdrawal("native", commitment, "auditor")
        .expect("challenge");

    let (_, info_r1_slashed) = bridge
        .relayer_status("r1", Some("native"))
        .expect("relayer status");
    let (_, info_r2_slashed) = bridge
        .relayer_status("r2", Some("native"))
        .expect("relayer status");
    assert!(info_r1_slashed.penalties_applied >= params.challenge_slash);
    assert!(info_r2_slashed.penalties_applied >= params.challenge_slash);
    assert!(info_r1_slashed.bond < info_r1_after.bond);
    assert!(info_r2_slashed.bond < info_r2_after.bond);

    let failed_records: Vec<_> = bridge
        .duty_log(None, Some("native"), 32)
        .into_iter()
        .filter(|record| matches!(record.status, DutyStatus::Failed { .. }))
        .collect();
    assert!(failed_records.iter().any(|record| record.relayer == "r1"));
    assert!(failed_records.iter().any(|record| record.relayer == "r2"));

    let (disputes, _) = bridge.dispute_audit(Some("native"), None, 256);
    let challenged_entry = disputes
        .iter()
        .find(|entry| entry.commitment == commitment)
        .expect("dispute entry present");
    assert!(challenged_entry.challenged);
    assert_eq!(challenged_entry.challenger.as_deref(), Some("auditor"));
    assert!(!challenged_entry.relayer_outcomes.is_empty());

    // Governance override: increase reward and ensure new duties use it.
    let mut updated = params.clone();
    updated.duty_reward = 40;
    let _override_guard = IncentiveGuard::set(updated.clone());
    let header = sample_header();
    let proof = sample_proof();
    let bundle = sample_bundle("bob", 10);
    bridge
        .deposit("native", "r1", "bob", 10, &header, &proof, &bundle)
        .expect("third deposit");
    let (_, info_r1_updated) = bridge
        .relayer_status("r1", Some("native"))
        .expect("relayer status");
    assert!(info_r1_updated.rewards_earned >= info_r1_slashed.rewards_earned + updated.duty_reward);
}

#[test]
fn reward_claim_requires_governance_approval() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
    configure_native_channel(&mut bridge, &headers_dir, 0);

    let params = BridgeIncentiveParameters {
        min_bond: 25,
        duty_reward: 15,
        failure_slash: 5,
        challenge_slash: 10,
        duty_window_secs: 90,
    };
    let _incentive_guard = IncentiveGuard::set(params.clone());

    bridge.bond_relayer("r1", 200).unwrap();
    bridge.bond_relayer("r2", 200).unwrap();

    let header = sample_header_with_height(5);
    let proof = sample_proof();
    let bundle = sample_bundle("dave", 70);
    bridge
        .deposit("native", "r1", "dave", 70, &header, &proof, &bundle)
        .expect("deposit");

    let (_, status) = bridge.relayer_status("r1", Some("native")).expect("status");
    assert_eq!(status.rewards_pending, params.duty_reward);

    let err = bridge
        .claim_rewards("r1", params.duty_reward, "bad-key")
        .unwrap_err();
    assert!(matches!(err, BridgeError::RewardClaimRejected(_)));

    let approval = RewardClaimApproval::new("approval-r1", "r1", params.duty_reward * 2);
    {
        let store = GovStore::open(&gov_path);
        store
            .record_reward_claim(&approval)
            .expect("record approval");
    }

    let claim_one = bridge
        .claim_rewards("r1", params.duty_reward, "approval-r1")
        .expect("claim approved rewards");
    assert_eq!(claim_one.amount, params.duty_reward);
    assert_eq!(claim_one.pending_after, 0);

    let (_, after_first_claim) = bridge.relayer_status("r1", Some("native")).expect("status");
    assert_eq!(after_first_claim.rewards_pending, 0);
    assert_eq!(after_first_claim.rewards_claimed, params.duty_reward);

    let header_two = sample_header_with_height(6);
    let proof_two = sample_proof();
    let bundle_two = sample_bundle("dave", 30);
    bridge
        .deposit(
            "native",
            "r1",
            "dave",
            30,
            &header_two,
            &proof_two,
            &bundle_two,
        )
        .expect("second deposit");

    let claim_two = bridge
        .claim_rewards("r1", params.duty_reward, "approval-r1")
        .expect("consume remaining approval");
    assert!(claim_two.id > claim_one.id);

    let (claims, next_cursor) = bridge.reward_claims(Some("r1"), None, 256);
    assert_eq!(claims.len(), 2);
    assert_eq!(claims[0].approval_key, "approval-r1");
    assert_eq!(claims[1].approval_key, "approval-r1");
    assert!(next_cursor.is_none());

    let (page_one, cursor) = bridge.reward_claims(Some("r1"), None, 1);
    assert_eq!(page_one.len(), 1);
    assert_eq!(cursor, Some(1));
    let (page_two, cursor_two) = bridge.reward_claims(Some("r1"), cursor, 1);
    assert_eq!(page_two.len(), 1);
    assert!(cursor_two.is_none());
    assert_ne!(page_one[0].id, page_two[0].id);

    {
        let store = GovStore::open(&gov_path);
        assert!(store.reward_claim("approval-r1").unwrap().is_none());
    }

    let header_three = sample_header_with_height(7);
    let proof_three = sample_proof();
    let bundle_three = sample_bundle("dave", 20);
    bridge
        .deposit(
            "native",
            "r1",
            "dave",
            20,
            &header_three,
            &proof_three,
            &bundle_three,
        )
        .expect("third deposit for rejection test");

    let err = bridge
        .claim_rewards("r1", params.duty_reward, "approval-r1")
        .unwrap_err();
    assert!(matches!(err, BridgeError::RewardClaimRejected(_)));

    let err = bridge
        .claim_rewards("r1", params.duty_reward, "missing-pending")
        .unwrap_err();
    assert!(matches!(err, BridgeError::RewardClaimRejected(_)));
}

#[test]
fn settlement_proof_flow_records_and_audits() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));

    let config = ChannelConfig {
        asset: "native".into(),
        confirm_depth: 1,
        fee_per_byte: 0,
        challenge_period_secs: 0,
        relayer_quorum: 2,
        headers_dir: headers_dir.to_str().expect("headers dir").to_string(),
        requires_settlement_proof: true,
        settlement_chain: Some("solana".into()),
    };
    bridge
        .set_channel_config("native", config)
        .expect("configure settlement channel");

    let params = BridgeIncentiveParameters {
        min_bond: 10,
        duty_reward: 8,
        failure_slash: 3,
        challenge_slash: 12,
        duty_window_secs: 60,
    };
    let _incentive_guard = IncentiveGuard::set(params.clone());

    bridge.bond_relayer("r1", 200).unwrap();
    bridge.bond_relayer("r2", 200).unwrap();

    let header = sample_header_with_height(7);
    let proof = sample_proof();
    let bundle = sample_bundle("erin", 40);
    bridge
        .deposit("native", "r1", "erin", 40, &header, &proof, &bundle)
        .expect("deposit");

    let commitment = bundle.aggregate_commitment("erin", 40);
    approve_release(&gov_path, "native", &commitment);
    let commitment = bridge
        .request_withdrawal("native", "r1", "erin", 40, &bundle)
        .expect("request withdrawal");

    let err = bridge
        .finalize_withdrawal("native", commitment)
        .unwrap_err();
    assert!(matches!(err, BridgeError::SettlementProofRequired { .. }));

    let wrong_hash = settlement_proof_digest(
        "native",
        &commitment,
        "ethereum",
        55,
        "erin",
        40,
        &bundle.relayer_ids(),
    );
    let wrong_proof = ExternalSettlementProof {
        commitment,
        settlement_chain: "ethereum".into(),
        proof_hash: wrong_hash,
        settlement_height: 55,
    };
    let err = bridge
        .submit_settlement_proof("native", "r1", wrong_proof)
        .unwrap_err();
    assert!(matches!(
        err,
        BridgeError::SettlementProofChainMismatch { .. }
    ));

    let correct_hash = settlement_proof_digest(
        "native",
        &commitment,
        "solana",
        60,
        "erin",
        40,
        &bundle.relayer_ids(),
    );
    let mut tampered_hash = correct_hash;
    tampered_hash[0] ^= 0xFF;
    let bad_hash_proof = ExternalSettlementProof {
        commitment,
        settlement_chain: "solana".into(),
        proof_hash: tampered_hash,
        settlement_height: 60,
    };
    let err = bridge
        .submit_settlement_proof("native", "r1", bad_hash_proof)
        .unwrap_err();
    assert!(matches!(
        err,
        BridgeError::SettlementProofHashMismatch { .. }
    ));

    let correct_proof = ExternalSettlementProof {
        commitment,
        settlement_chain: "solana".into(),
        proof_hash: correct_hash,
        settlement_height: 60,
    };
    let record = bridge
        .submit_settlement_proof("native", "r1", correct_proof.clone())
        .expect("submit settlement proof");
    assert_eq!(record.settlement_chain.as_deref(), Some("solana"));

    let duplicate_err = bridge
        .submit_settlement_proof("native", "r1", correct_proof.clone())
        .unwrap_err();
    assert!(matches!(
        duplicate_err,
        BridgeError::SettlementProofHeightReplay { .. }
    ));

    let pending = bridge.pending_withdrawals(Some("native"));
    assert_eq!(pending.len(), 1);
    let entry = &pending[0];
    assert!(entry.requires_settlement_proof);
    assert_eq!(entry.settlement_chain.as_deref(), Some("solana"));
    assert!(entry.settlement_submitted_at.is_some());

    let (settlements, _) = bridge.settlement_records(Some("native"), None, 256);
    assert_eq!(settlements.len(), 1);
    assert_eq!(settlements[0].proof_hash, correct_hash);

    let header_two = sample_header_with_height(8);
    let proof_two = sample_proof();
    let bundle_two = sample_bundle("frank", 30);
    bridge
        .deposit(
            "native",
            "r1",
            "frank",
            30,
            &header_two,
            &proof_two,
            &bundle_two,
        )
        .expect("second settlement deposit");
    let second_commitment_key = bundle_two.aggregate_commitment("frank", 30);
    approve_release(&gov_path, "native", &second_commitment_key);
    let second_commitment = bridge
        .request_withdrawal("native", "r1", "frank", 30, &bundle_two)
        .expect("second withdrawal request");
    let earlier_hash = settlement_proof_digest(
        "native",
        &second_commitment,
        "solana",
        58,
        "frank",
        30,
        &bundle_two.relayer_ids(),
    );
    let earlier_proof = ExternalSettlementProof {
        commitment: second_commitment,
        settlement_chain: "solana".into(),
        proof_hash: earlier_hash,
        settlement_height: 58,
    };
    let second_record = bridge
        .submit_settlement_proof("native", "r1", earlier_proof)
        .expect("accept out-of-order settlement proof");
    assert_eq!(second_record.commitment, second_commitment);
    assert_eq!(second_record.settlement_height, 58);
    let second_hash = settlement_proof_digest(
        "native",
        &second_commitment,
        "solana",
        66,
        "frank",
        30,
        &bundle_two.relayer_ids(),
    );
    let second_proof = ExternalSettlementProof {
        commitment: second_commitment,
        settlement_chain: "solana".into(),
        proof_hash: second_hash,
        settlement_height: 66,
    };
    let duplicate_err = bridge
        .submit_settlement_proof("native", "r1", second_proof)
        .unwrap_err();
    assert!(matches!(
        duplicate_err,
        BridgeError::SettlementProofDuplicate
    ));

    let (first_page, cursor) = bridge.settlement_records(Some("native"), None, 1);
    assert_eq!(first_page.len(), 1);
    let (second_page, cursor_two) = bridge.settlement_records(Some("native"), cursor, 1);
    assert_eq!(second_page.len(), 1);
    assert!(cursor_two.is_none());
    assert_ne!(first_page[0].commitment, second_page[0].commitment);
    assert_eq!(second_page[0].commitment, second_commitment);

    let (disputes, _) = bridge.dispute_audit(Some("native"), None, 256);
    assert_eq!(disputes.len(), 2);
    let (disputes_page_one, dispute_cursor) = bridge.dispute_audit(Some("native"), None, 1);
    assert_eq!(disputes_page_one.len(), 1);
    assert_eq!(dispute_cursor, Some(1));
    let (disputes_page_two, dispute_cursor_two) =
        bridge.dispute_audit(Some("native"), dispute_cursor, 1);
    assert_eq!(disputes_page_two.len(), 1);
    assert!(dispute_cursor_two.is_none());
    assert!(disputes_page_two
        .iter()
        .any(|record| record.commitment == second_commitment));
    assert_ne!(
        disputes_page_one[0].commitment,
        disputes_page_two[0].commitment
    );
    let dispute = disputes
        .iter()
        .find(|record| record.commitment == commitment)
        .expect("dispute entry");
    assert!(dispute.settlement_required);
    assert_eq!(dispute.settlement_chain.as_deref(), Some("solana"));
    assert!(dispute.settlement_submitted_at.is_some());

    bridge
        .finalize_withdrawal("native", commitment)
        .expect("finalize with proof");

    bridge
        .finalize_withdrawal("native", second_commitment)
        .expect("finalize second settlement");

    let pending_after = bridge.pending_withdrawals(Some("native"));
    assert!(pending_after.is_empty());

    let (disputes_after, _) = bridge.dispute_audit(Some("native"), None, 256);
    let entry_after = disputes_after
        .iter()
        .find(|record| record.commitment == commitment)
        .expect("dispute entry after finalize");
    assert!(entry_after
        .relayer_outcomes
        .iter()
        .any(|outcome| outcome.status == "completed"));

    let second_entry_after = disputes_after
        .iter()
        .find(|record| record.commitment == second_commitment)
        .expect("second dispute entry after finalize");
    assert!(second_entry_after
        .relayer_outcomes
        .iter()
        .any(|outcome| outcome.status == "completed"));

    let (accruals, accrual_cursor) = bridge.reward_accruals(None, Some("native"), None, 256);
    assert!(accruals.len() >= 6);
    assert!(accruals
        .iter()
        .any(|record| record.duty_kind == "deposit" && record.relayer == "r1"));
    assert!(accruals.iter().any(|record| {
        record.duty_kind == "settlement"
            && record.commitment == Some(commitment)
            && record.proof_hash == Some(correct_hash)
            && record.bundle_relayers.contains(&"r1".to_string())
            && record.bundle_relayers.contains(&"r2".to_string())
    }));
    assert!(accruals
        .iter()
        .filter(|record| record.duty_kind == "withdrawal")
        .any(|record| record.commitment == Some(commitment)));
    assert!(accruals
        .iter()
        .filter(|record| record.duty_kind == "withdrawal")
        .any(|record| record.commitment == Some(second_commitment)));
    assert!(accruals.iter().all(|record| record.asset == "native"));
    assert!(accruals.iter().all(|record| record.recorded_at > 0));
    assert!(accrual_cursor.is_none());

    let (page_one, accrual_cursor) = bridge.reward_accruals(None, Some("native"), None, 1);
    assert_eq!(page_one.len(), 1);
    assert!(accrual_cursor.is_some());
    let (page_two, _accrual_cursor_two) =
        bridge.reward_accruals(None, Some("native"), accrual_cursor, 1);
    assert_eq!(page_two.len(), 1);
    assert_ne!(page_one[0].id, page_two[0].id);
    let (r2_accruals, _) = bridge.reward_accruals(Some("r2"), Some("native"), None, 64);
    assert!(!r2_accruals.is_empty());
    assert!(r2_accruals
        .iter()
        .all(|record| record.relayer == "r2" && record.duty_kind == "withdrawal"));
}

#[cfg(feature = "test-telemetry")]
#[test]
fn telemetry_tracks_bridge_flows() {
    BRIDGE_REWARD_CLAIMS_TOTAL.reset();
    BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL.reset();
    let settlement_success = BRIDGE_SETTLEMENT_RESULTS_TOTAL
        .ensure_handle_for_label_values(&["success", "ok"])
        .expect(LABEL_REGISTRATION_ERR);
    settlement_success.reset();
    let settlement_chain_mismatch = BRIDGE_SETTLEMENT_RESULTS_TOTAL
        .ensure_handle_for_label_values(&["failure", "chain_mismatch"])
        .expect(LABEL_REGISTRATION_ERR);
    settlement_chain_mismatch.reset();
    let settlement_hash_mismatch = BRIDGE_SETTLEMENT_RESULTS_TOTAL
        .ensure_handle_for_label_values(&["failure", "hash_mismatch"])
        .expect(LABEL_REGISTRATION_ERR);
    settlement_hash_mismatch.reset();
    let settlement_duplicate = BRIDGE_SETTLEMENT_RESULTS_TOTAL
        .ensure_handle_for_label_values(&["failure", "duplicate"])
        .expect(LABEL_REGISTRATION_ERR);
    settlement_duplicate.reset();
    let settlement_height_replay = BRIDGE_SETTLEMENT_RESULTS_TOTAL
        .ensure_handle_for_label_values(&["failure", "height_replay"])
        .expect(LABEL_REGISTRATION_ERR);
    settlement_height_replay.reset();
    let dispute_settlement_success = BRIDGE_DISPUTE_OUTCOMES_TOTAL
        .ensure_handle_for_label_values(&["settlement", "success"])
        .expect(LABEL_REGISTRATION_ERR);
    dispute_settlement_success.reset();
    let dispute_withdrawal_challenge = BRIDGE_DISPUTE_OUTCOMES_TOTAL
        .ensure_handle_for_label_values(&["withdrawal", "challenge_accepted"])
        .expect(LABEL_REGISTRATION_ERR);
    dispute_withdrawal_challenge.reset();

    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));

    let config = ChannelConfig {
        asset: "native".into(),
        confirm_depth: 1,
        fee_per_byte: 0,
        challenge_period_secs: 0,
        relayer_quorum: 2,
        headers_dir: headers_dir.to_str().expect("headers dir").to_string(),
        requires_settlement_proof: true,
        settlement_chain: Some("solana".into()),
    };
    bridge
        .set_channel_config("native", config)
        .expect("configure settlement channel");

    let params = BridgeIncentiveParameters {
        min_bond: 25,
        duty_reward: 12,
        failure_slash: 4,
        challenge_slash: 11,
        duty_window_secs: 45,
    };
    let _incentive_guard = IncentiveGuard::set(params.clone());

    bridge.bond_relayer("r1", 200).unwrap();
    bridge.bond_relayer("r2", 200).unwrap();

    let header = sample_header_with_height(10);
    let proof = sample_proof();
    let bundle = sample_bundle("erin", 40);
    bridge
        .deposit("native", "r1", "erin", 40, &header, &proof, &bundle)
        .expect("deposit");

    let commitment = bundle.aggregate_commitment("erin", 40);
    approve_release(&gov_path, "native", &commitment);
    let commitment = bridge
        .request_withdrawal("native", "r1", "erin", 40, &bundle)
        .expect("request withdrawal");

    let failure_before = settlement_chain_mismatch.get();
    let wrong_hash = settlement_proof_digest(
        "native",
        &commitment,
        "ethereum",
        51,
        "erin",
        40,
        &bundle.relayer_ids(),
    );
    let wrong_proof = ExternalSettlementProof {
        commitment,
        settlement_chain: "ethereum".into(),
        proof_hash: wrong_hash,
        settlement_height: 51,
    };
    let err = bridge
        .submit_settlement_proof("native", "r1", wrong_proof)
        .unwrap_err();
    assert!(matches!(
        err,
        BridgeError::SettlementProofChainMismatch { .. }
    ));
    assert_eq!(settlement_chain_mismatch.get(), failure_before + 1);

    let success_before = settlement_success.get();
    let dispute_settlement_before = dispute_settlement_success.get();
    let hash_failure_before = settlement_hash_mismatch.get();
    let correct_hash = settlement_proof_digest(
        "native",
        &commitment,
        "solana",
        52,
        "erin",
        40,
        &bundle.relayer_ids(),
    );
    let mut tampered_hash = correct_hash;
    tampered_hash[2] ^= 0xAA;
    let bad_hash_proof = ExternalSettlementProof {
        commitment,
        settlement_chain: "solana".into(),
        proof_hash: tampered_hash,
        settlement_height: 52,
    };
    let err = bridge
        .submit_settlement_proof("native", "r1", bad_hash_proof)
        .unwrap_err();
    assert!(matches!(
        err,
        BridgeError::SettlementProofHashMismatch { .. }
    ));
    assert_eq!(settlement_hash_mismatch.get(), hash_failure_before + 1);

    let correct_proof = ExternalSettlementProof {
        commitment,
        settlement_chain: "solana".into(),
        proof_hash: correct_hash,
        settlement_height: 52,
    };
    let record = bridge
        .submit_settlement_proof("native", "r1", correct_proof)
        .expect("submit settlement proof");
    assert_eq!(record.settlement_chain.as_deref(), Some("solana"));
    assert_eq!(settlement_success.get(), success_before + 1);
    assert_eq!(
        dispute_settlement_success.get(),
        dispute_settlement_before + 1
    );

    let approval = RewardClaimApproval::new("approval-r1", "r1", params.duty_reward * 2);
    {
        let store = GovStore::open(&gov_path);
        store
            .record_reward_claim(&approval)
            .expect("record approval");
    }

    let reward_claims_before = BRIDGE_REWARD_CLAIMS_TOTAL.value();
    let approvals_before = BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL.value();
    let claim = bridge
        .claim_rewards("r1", params.duty_reward, "approval-r1")
        .expect("claim approved rewards");
    assert_eq!(claim.amount, params.duty_reward);
    assert_eq!(BRIDGE_REWARD_CLAIMS_TOTAL.value(), reward_claims_before + 1);
    assert_eq!(
        BRIDGE_REWARD_APPROVALS_CONSUMED_TOTAL.value(),
        approvals_before + params.duty_reward
    );

    let header_two = sample_header_with_height(11);
    let proof_two = sample_proof();
    let bundle_two = sample_bundle("erin", 10);
    bridge
        .deposit(
            "native",
            "r2",
            "erin",
            10,
            &header_two,
            &proof_two,
            &bundle_two,
        )
        .expect("second deposit");
    let commitment_two = bundle_two.aggregate_commitment("erin", 10);
    approve_release(&gov_path, "native", &commitment_two);
    let commitment_two = bridge
        .request_withdrawal("native", "r2", "erin", 10, &bundle_two)
        .expect("second withdrawal");
    let duplicate_before = settlement_duplicate.get();
    let replay_before = settlement_height_replay.get();
    let dispute_challenge_before = dispute_withdrawal_challenge.get();
    bridge
        .challenge_withdrawal("native", commitment_two, "auditor")
        .expect("challenge withdrawal");
    let expected_failures = {
        let mut set = std::collections::HashSet::new();
        for relayer in bundle_two.relayer_ids() {
            set.insert(relayer);
        }
        set.len() as u64
    };
    assert_eq!(
        dispute_withdrawal_challenge.get(),
        dispute_challenge_before + expected_failures
    );

    let header_three = sample_header_with_height(12);
    let proof_three = sample_proof();
    let bundle_three = sample_bundle("erin", 8);
    bridge
        .deposit(
            "native",
            "r1",
            "erin",
            8,
            &header_three,
            &proof_three,
            &bundle_three,
        )
        .expect("third deposit for duplicate test");
    let commitment_three = bundle_three.aggregate_commitment("erin", 8);
    approve_release(&gov_path, "native", &commitment_three);
    let commitment_three = bridge
        .request_withdrawal("native", "r1", "erin", 8, &bundle_three)
        .expect("third withdrawal");
    let accepted_hash = settlement_proof_digest(
        "native",
        &commitment_three,
        "solana",
        58,
        "erin",
        8,
        &bundle_three.relayer_ids(),
    );
    let accepted_proof = ExternalSettlementProof {
        commitment: commitment_three,
        settlement_chain: "solana".into(),
        proof_hash: accepted_hash,
        settlement_height: 58,
    };
    bridge
        .submit_settlement_proof("native", "r1", accepted_proof)
        .expect("accept settlement proof for third commitment");
    let duplicate_attempt = ExternalSettlementProof {
        commitment: commitment_three,
        settlement_chain: "solana".into(),
        proof_hash: settlement_proof_digest(
            "native",
            &commitment_three,
            "solana",
            75,
            "erin",
            8,
            &bundle_three.relayer_ids(),
        ),
        settlement_height: 75,
    };
    let err = bridge
        .submit_settlement_proof("native", "r1", duplicate_attempt)
        .unwrap_err();
    assert!(matches!(err, BridgeError::SettlementProofDuplicate));
    assert_eq!(settlement_duplicate.get(), duplicate_before + 1);
    assert_eq!(settlement_height_replay.get(), replay_before);
}

#[test]
fn bridge_audit_pagination_handles_cursor_edge_cases() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));

    let config = ChannelConfig {
        asset: "native".into(),
        confirm_depth: 1,
        fee_per_byte: 0,
        challenge_period_secs: 0,
        relayer_quorum: 2,
        headers_dir: headers_dir.to_str().expect("headers dir").to_string(),
        requires_settlement_proof: true,
        settlement_chain: Some("solana".into()),
    };
    bridge
        .set_channel_config("native", config)
        .expect("configure settlement channel");

    let params = BridgeIncentiveParameters {
        min_bond: 25,
        duty_reward: 12,
        failure_slash: 4,
        challenge_slash: 11,
        duty_window_secs: 45,
    };
    let _incentive_guard = IncentiveGuard::set(params);

    bridge.bond_relayer("r1", 200).unwrap();
    bridge.bond_relayer("r2", 200).unwrap();

    let scenarios = [("amy", 20u64, 60u64, 14u64), ("beth", 15u64, 58u64, 18u64)];
    for (user, amount, settlement_height, header_height) in scenarios {
        let header = sample_header_with_height(header_height);
        let proof = sample_proof();
        let bundle = sample_bundle(user, amount);
        bridge
            .deposit("native", "r1", user, amount, &header, &proof, &bundle)
            .expect("deposit");
        let commitment_key = bundle.aggregate_commitment(user, amount);
        approve_release(&gov_path, "native", &commitment_key);
        let commitment = bridge
            .request_withdrawal("native", "r1", user, amount, &bundle)
            .expect("request withdrawal");
        let proof_hash = settlement_proof_digest(
            "native",
            &commitment,
            "solana",
            settlement_height,
            user,
            amount,
            &bundle.relayer_ids(),
        );
        let settlement_proof = ExternalSettlementProof {
            commitment,
            settlement_chain: "solana".into(),
            proof_hash,
            settlement_height,
        };
        bridge
            .submit_settlement_proof("native", "r1", settlement_proof)
            .expect("submit settlement proof");
    }

    let (all_accruals, _) = bridge.reward_accruals(None, Some("native"), None, 256);
    assert!(all_accruals.len() >= 4);
    let (accrual_page, accrual_cursor) = bridge.reward_accruals(None, Some("native"), None, 1);
    assert_eq!(accrual_page.len(), 1);
    assert!(accrual_cursor.is_some());
    let (accrual_zero_cursor, accrual_zero_next) =
        bridge.reward_accruals(None, Some("native"), Some(0), 1);
    assert_eq!(accrual_zero_cursor.len(), 1);
    assert_eq!(accrual_zero_cursor[0].id, accrual_page[0].id);
    assert_eq!(accrual_zero_next, accrual_cursor);
    let (accrual_zero_limit, accrual_zero_limit_next) =
        bridge.reward_accruals(None, Some("native"), Some(0), 0);
    assert_eq!(accrual_zero_limit.len(), 1);
    assert_eq!(accrual_zero_limit_next, accrual_cursor);
    let overflow_start = all_accruals.len() as u64 + 5;
    let (accrual_overflow, accrual_overflow_cursor) =
        bridge.reward_accruals(None, Some("native"), Some(overflow_start), 1);
    assert!(accrual_overflow.is_empty());
    assert!(accrual_overflow_cursor.is_none());

    let (settlement_page, settlement_cursor) = bridge.settlement_records(Some("native"), None, 1);
    assert_eq!(settlement_page.len(), 1);
    assert_eq!(settlement_cursor, Some(1));
    let (settlement_zero_cursor, settlement_zero_next) =
        bridge.settlement_records(Some("native"), Some(0), 1);
    assert_eq!(
        settlement_zero_cursor[0].commitment,
        settlement_page[0].commitment
    );
    assert_eq!(settlement_zero_next, settlement_cursor);
    let (settlement_zero_limit, settlement_zero_limit_next) =
        bridge.settlement_records(Some("native"), Some(0), 0);
    assert_eq!(settlement_zero_limit.len(), 1);
    assert_eq!(settlement_zero_limit_next, settlement_cursor);
    let all_settlements = bridge.settlement_records(Some("native"), None, 256).0;
    assert!(all_settlements.len() >= 2);
    let settlement_overflow_cursor_value = all_settlements.len() as u64 + 3;
    let (settlement_overflow, settlement_overflow_cursor) =
        bridge.settlement_records(Some("native"), Some(settlement_overflow_cursor_value), 1);
    assert!(settlement_overflow.is_empty());
    assert!(settlement_overflow_cursor.is_none());

    let (disputes_page, disputes_cursor) = bridge.dispute_audit(Some("native"), None, 1);
    assert_eq!(disputes_page.len(), 1);
    assert!(disputes_cursor.is_some());
    let (disputes_zero_cursor, disputes_zero_next) =
        bridge.dispute_audit(Some("native"), Some(0), 1);
    assert_eq!(
        disputes_zero_cursor[0].commitment,
        disputes_page[0].commitment
    );
    assert_eq!(disputes_zero_next, disputes_cursor);
    let (disputes_zero_limit, disputes_zero_limit_next) =
        bridge.dispute_audit(Some("native"), Some(0), 0);
    assert_eq!(disputes_zero_limit.len(), 1);
    assert_eq!(disputes_zero_limit_next, disputes_cursor);
    let all_disputes = bridge.dispute_audit(Some("native"), None, 256).0;
    assert!(all_disputes.len() >= 2);
    let dispute_overflow_cursor_value = all_disputes.len() as u64 + 7;
    let (disputes_overflow, disputes_overflow_cursor) =
        bridge.dispute_audit(Some("native"), Some(dispute_overflow_cursor_value), 1);
    assert!(disputes_overflow.is_empty());
    assert!(disputes_overflow_cursor.is_none());
}

#[test]
fn bridge_restart_restores_accrual_and_dispute_history() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db");
    let headers_dir = tmp.path().join("headers_native");

    let params = BridgeIncentiveParameters {
        min_bond: 25,
        duty_reward: 12,
        failure_slash: 4,
        challenge_slash: 11,
        duty_window_secs: 45,
    };
    let _incentive_guard = IncentiveGuard::set(params);

    let first_commitment = {
        let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
        let config = ChannelConfig {
            asset: "native".into(),
            confirm_depth: 1,
            fee_per_byte: 0,
            challenge_period_secs: 0,
            relayer_quorum: 2,
            headers_dir: headers_dir.to_str().expect("headers dir").to_string(),
            requires_settlement_proof: true,
            settlement_chain: Some("solana".into()),
        };
        bridge
            .set_channel_config("native", config)
            .expect("configure settlement channel");
        bridge.bond_relayer("r1", 200).unwrap();
        bridge.bond_relayer("r2", 200).unwrap();

        let header = sample_header_with_height(20);
        let proof = sample_proof();
        let bundle = sample_bundle("ivy", 30);
        bridge
            .deposit("native", "r1", "ivy", 30, &header, &proof, &bundle)
            .expect("deposit");
        let commitment_key = bundle.aggregate_commitment("ivy", 30);
        approve_release(&gov_path, "native", &commitment_key);
        let commitment = bridge
            .request_withdrawal("native", "r1", "ivy", 30, &bundle)
            .expect("request withdrawal");
        let proof_hash = settlement_proof_digest(
            "native",
            &commitment,
            "solana",
            60,
            "ivy",
            30,
            &bundle.relayer_ids(),
        );
        let settlement_proof = ExternalSettlementProof {
            commitment,
            settlement_chain: "solana".into(),
            proof_hash,
            settlement_height: 60,
        };
        bridge
            .submit_settlement_proof("native", "r1", settlement_proof)
            .expect("submit settlement proof before restart");
        commitment
    };

    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
    let (settlements_after_restart, _) = bridge.settlement_records(Some("native"), None, 256);
    assert_eq!(settlements_after_restart.len(), 1);
    assert_eq!(settlements_after_restart[0].commitment, first_commitment);
    let (disputes_after_restart, _) = bridge.dispute_audit(Some("native"), None, 256);
    assert!(disputes_after_restart
        .iter()
        .any(|record| record.commitment == first_commitment));
    let (accruals_after_restart, _) = bridge.reward_accruals(None, Some("native"), None, 256);
    assert!(accruals_after_restart
        .iter()
        .any(|record| record.commitment == Some(first_commitment)));

    let header_two = sample_header_with_height(24);
    let proof_two = sample_proof();
    let bundle_two = sample_bundle("jill", 18);
    bridge
        .deposit(
            "native",
            "r1",
            "jill",
            18,
            &header_two,
            &proof_two,
            &bundle_two,
        )
        .expect("second deposit after restart");
    let commitment_key_two = bundle_two.aggregate_commitment("jill", 18);
    approve_release(&gov_path, "native", &commitment_key_two);
    let commitment_two = bridge
        .request_withdrawal("native", "r1", "jill", 18, &bundle_two)
        .expect("second withdrawal after restart");
    let out_of_order_hash = settlement_proof_digest(
        "native",
        &commitment_two,
        "solana",
        58,
        "jill",
        18,
        &bundle_two.relayer_ids(),
    );
    let out_of_order_proof = ExternalSettlementProof {
        commitment: commitment_two,
        settlement_chain: "solana".into(),
        proof_hash: out_of_order_hash,
        settlement_height: 58,
    };
    let record_after_restart = bridge
        .submit_settlement_proof("native", "r1", out_of_order_proof)
        .expect("accept settlement proof with lower height after restart");
    assert_eq!(record_after_restart.settlement_height, 58);

    let (settlement_pages, cursor) = bridge.settlement_records(Some("native"), None, 2);
    assert_eq!(settlement_pages.len(), 2);
    assert!(cursor.is_none());
    assert!(settlement_pages
        .iter()
        .any(|record| record.commitment == first_commitment));
    assert!(settlement_pages
        .iter()
        .any(|record| record.commitment == commitment_two));
}

#[test]
fn bridge_incentive_summary_exposes_pending_rewards_and_duties() {
    let tmp = tempdir().expect("tempdir");
    let gov_path = tmp.path().join("gov");
    let _guard = GovEnvGuard::set(&gov_path);

    let bridge_path = tmp.path().join("bridge_db_summary");
    let headers_dir = tmp.path().join("headers_native_summary");
    let mut bridge = Bridge::open(bridge_path.to_str().expect("bridge path"));
    configure_native_channel(&mut bridge, &headers_dir, 0);

    let params = BridgeIncentiveParameters {
        min_bond: 10,
        duty_reward: 15,
        failure_slash: 5,
        challenge_slash: 10,
        duty_window_secs: 120,
    };
    let _incentive_guard = IncentiveGuard::set(params.clone());

    bridge.bond_relayer("r1", 100).unwrap();
    bridge.bond_relayer("r2", 100).unwrap();

    let header = sample_header_with_height(2);
    let proof = sample_proof();
    let bundle = sample_bundle("alice", 5);

    bridge
        .deposit("native", "r1", "alice", 5, &header, &proof, &bundle)
        .expect("deposit");
    let commitment = bundle.aggregate_commitment("alice", 5);
    approve_release(&gov_path, "native", &commitment);

    let pending_commitment = bridge
        .request_withdrawal("native", "r1", "alice", 5, &bundle)
        .expect("withdraw request");

    let summaries = bridge.incentive_summary();
    let summary = summaries
        .into_iter()
        .find(|entry| entry.asset == "native")
        .expect("native summary");
    assert_eq!(summary.claimable_rewards, params.duty_reward);
    assert!(summary.pending_duties >= 1);
    assert_eq!(summary.active_relayers, 2);
    assert!(summary.receipt_count >= 1);

    bridge
        .finalize_withdrawal("native", pending_commitment)
        .expect("finalize");
}
