#![allow(clippy::unwrap_used, clippy::expect_used)]

use bridge_types::{BridgeIncentiveParameters, DutyStatus, ExternalSettlementProof};
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

    let wrong_proof = ExternalSettlementProof {
        commitment,
        settlement_chain: "ethereum".into(),
        proof_hash: [1u8; 32],
        settlement_height: 55,
    };
    let err = bridge
        .submit_settlement_proof("native", "r1", wrong_proof)
        .unwrap_err();
    assert!(matches!(
        err,
        BridgeError::SettlementProofChainMismatch { .. }
    ));

    let correct_proof = ExternalSettlementProof {
        commitment,
        settlement_chain: "solana".into(),
        proof_hash: [2u8; 32],
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
        BridgeError::SettlementProofDuplicate
    ));

    let pending = bridge.pending_withdrawals(Some("native"));
    assert_eq!(pending.len(), 1);
    let entry = &pending[0];
    assert!(entry.requires_settlement_proof);
    assert_eq!(entry.settlement_chain.as_deref(), Some("solana"));
    assert!(entry.settlement_submitted_at.is_some());

    let (settlements, _) = bridge.settlement_records(Some("native"), None, 256);
    assert_eq!(settlements.len(), 1);
    assert_eq!(settlements[0].proof_hash, correct_proof.proof_hash);

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
    let second_proof = ExternalSettlementProof {
        commitment: second_commitment,
        settlement_chain: "solana".into(),
        proof_hash: [3u8; 32],
        settlement_height: 66,
    };
    bridge
        .submit_settlement_proof("native", "r1", second_proof)
        .expect("submit second settlement proof");

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
    let wrong_proof = ExternalSettlementProof {
        commitment,
        settlement_chain: "ethereum".into(),
        proof_hash: [3u8; 32],
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
    let correct_proof = ExternalSettlementProof {
        commitment,
        settlement_chain: "solana".into(),
        proof_hash: [4u8; 32],
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
}
