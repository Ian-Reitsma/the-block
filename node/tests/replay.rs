use std::collections::HashSet;
use std::error::Error;
use std::sync::{Arc, Mutex};

use sys::tempfile::tempdir;
use the_block::{
    block_binary,
    governance::{
        controller,
        treasury::{
            canonical_dependencies, validate_dependencies, DisbursementDetails,
            DisbursementPayload, DisbursementProposalMetadata, DisbursementStatus,
        },
        GovStore, ParamKey, Params, Proposal, ProposalStatus, Runtime, Vote, VoteChoice,
    },
    receipts::{BlockTorchReceiptMetadata, ComputeReceipt, StorageReceipt},
    Account, Block, Blockchain, Receipt, TokenBalance,
};

type TestResult<T> = Result<T, Box<dyn Error>>;

fn treasury_event_block() -> Block {
    let receipt = Receipt::Storage(StorageReceipt {
        contract_id: "contract_1".into(),
        provider: "provider_1".into(),
        bytes: 1024,
        price: 10,
        block_height: 1,
        provider_escrow: 50,
        region: None,
        chunk_hash: None,
        provider_signature: vec![0u8; 64],
        signature_nonce: 0,
    });

    let mut block = Block {
        index: 1,
        receipts: vec![receipt],
        ..Default::default()
    };

    block.treasury_events.push(the_block::BlockTreasuryEvent {
        disbursement_id: 42,
        destination: "tb1replaydest".into(),
        amount: 1234,
        memo: "determinism-check".into(),
        scheduled_epoch: 5,
        tx_hash: "0xdeadbeef".into(),
        executed_at: 999,
    });

    block
}

#[test]
fn replay_roundtrip_block_with_receipts() {
    let receipt = Receipt::Storage(StorageReceipt {
        contract_id: "contract_1".into(),
        provider: "provider_1".into(),
        bytes: 1024,
        price: 10,
        block_height: 1,
        provider_escrow: 50,
        region: None,
        chunk_hash: None,
        provider_signature: vec![0u8; 64],
        signature_nonce: 0,
    });

    let block = Block {
        index: 1,
        receipts: vec![receipt],
        ..Default::default()
    };

    let encoded = block_binary::encode_block(&block).expect("encode block");
    let decoded = block_binary::decode_block(&encoded).expect("decode block");

    assert_eq!(block, decoded);
}

#[test]
fn replay_roundtrip_block_with_compute_receipt_blocktorch_metadata() {
    let receipt = Receipt::Compute(ComputeReceipt {
        job_id: "blocktorch-job".into(),
        provider: "blocktorch-provider".into(),
        compute_units: 10,
        payment: 50,
        block_height: 1,
        verified: true,
        blocktorch: Some(BlockTorchReceiptMetadata {
            kernel_variant_digest: [0xAA; 32],
            descriptor_digest: [0xBB; 32],
            output_digest: [0xCC; 32],
            benchmark_commit: Some("bench-commit".into()),
            tensor_profile_epoch: Some("epoch-1".into()),
            proof_latency_ms: 123,
        }),
        provider_signature: vec![0u8; 64],
        signature_nonce: 1,
    });

    let block = Block {
        index: 2,
        receipts: vec![receipt],
        ..Default::default()
    };

    let encoded = block_binary::encode_block(&block).expect("encode block");
    let decoded = block_binary::decode_block(&encoded).expect("decode block");

    assert_eq!(block, decoded);
}

#[test]
fn replay_roundtrip_block_compute_receipt_metadata_integrity() {
    let receipt = Receipt::Compute(ComputeReceipt {
        job_id: "bt-job-42".into(),
        provider: "bt-provider".into(),
        compute_units: 5,
        payment: 15,
        block_height: 10,
        verified: true,
        blocktorch: Some(BlockTorchReceiptMetadata {
            kernel_variant_digest: [0xDD; 32],
            descriptor_digest: [0xEE; 32],
            output_digest: [0xFF; 32],
            benchmark_commit: Some("bench-42".into()),
            tensor_profile_epoch: Some("epoch-42".into()),
            proof_latency_ms: 4242,
        }),
        provider_signature: vec![1, 2, 3],
        signature_nonce: 2,
    });

    let block = Block {
        index: 3,
        receipts: vec![receipt.clone()],
        ..Default::default()
    };

    let encoded = block_binary::encode_block(&block).expect("encode block");
    let decoded = block_binary::decode_block(&encoded).expect("decode block");

    let compute_receipt = match &decoded.receipts[0] {
        Receipt::Compute(r) => r,
        other => panic!("expected compute receipt, got {other:?}"),
    };
    let meta = compute_receipt
        .blocktorch
        .as_ref()
        .expect("blocktorch metadata missing");
    assert_eq!(meta.kernel_variant_digest, [0xDD; 32]);
    assert_eq!(meta.descriptor_digest, [0xEE; 32]);
    assert_eq!(meta.output_digest, [0xFF; 32]);
    assert_eq!(meta.benchmark_commit.as_deref(), Some("bench-42"));
    assert_eq!(meta.tensor_profile_epoch.as_deref(), Some("epoch-42"));
    assert_eq!(meta.proof_latency_ms, 4242);
    assert_eq!(block, decoded);
}

#[test]
fn replay_roundtrip_block_with_blocktorch_proof_loop_receipt() {
    let receipt = Receipt::Compute(ComputeReceipt {
        job_id: "bt-proof-job".into(),
        provider: "bt-proof-provider".into(),
        compute_units: 42,
        payment: 420,
        block_height: 5,
        verified: true,
        blocktorch: Some(BlockTorchReceiptMetadata {
            kernel_variant_digest: [0x11; 32],
            descriptor_digest: [0x22; 32],
            output_digest: [0x33; 32],
            benchmark_commit: Some("bench-proof".into()),
            tensor_profile_epoch: Some("epoch-proof".into()),
            proof_latency_ms: 9001,
        }),
        provider_signature: vec![9; 64],
        signature_nonce: 7,
    });

    let block = Block {
        index: 10,
        receipts: vec![receipt],
        ..Default::default()
    };

    let encoded = block_binary::encode_block(&block).expect("encode block");
    let decoded = block_binary::decode_block(&encoded).expect("decode block");

    assert_eq!(block, decoded);
    if let Receipt::Compute(compute) = &decoded.receipts[0] {
        let meta = compute
            .blocktorch
            .as_ref()
            .expect("BlockTorch metadata should survive");
        assert_eq!(meta.proof_latency_ms, 9001);
    } else {
        panic!("expected compute receipt");
    }
}

#[test]
fn replay_roundtrip_block_with_treasury_events() {
    let block = treasury_event_block();
    let encoded = block_binary::encode_block(&block).expect("encode block");
    let decoded = block_binary::decode_block(&encoded).expect("decode block");
    assert_eq!(block, decoded);
}

#[test]
fn replay_persists_dependency_dag_across_restart() -> TestResult<()> {
    let dir = tempdir()?;
    let db_path = dir.path().join("gov.db");

    {
        let store = GovStore::open(&db_path);
        store.record_treasury_accrual(5_000)?;

        let root = store.queue_disbursement(DisbursementPayload {
            proposal: DisbursementProposalMetadata::default(),
            disbursement: DisbursementDetails {
                destination: "tb1root".into(),
                amount: 250,
                memo: "{}".into(),
                scheduled_epoch: 1,
                expected_receipts: Vec::new(),
            },
        })?;

        let child = store.queue_disbursement(DisbursementPayload {
            proposal: DisbursementProposalMetadata {
                deps: vec![root.id],
                ..Default::default()
            },
            disbursement: DisbursementDetails {
                destination: "tb1child".into(),
                amount: 250,
                memo: format!("depends_on={}", root.id),
                scheduled_epoch: 2,
                expected_receipts: Vec::new(),
            },
        })?;

        let leaf = store.queue_disbursement(DisbursementPayload {
            proposal: DisbursementProposalMetadata {
                deps: vec![child.id],
                ..Default::default()
            },
            disbursement: DisbursementDetails {
                destination: "tb1leaf".into(),
                amount: 250,
                memo: format!("depends_on={}", child.id),
                scheduled_epoch: 3,
                expected_receipts: Vec::new(),
            },
        })?;
        let _leaf_id = leaf.id;

        // Roll back the middle disbursement to create a dependency failure chain.
        store.cancel_disbursement(child.id, "mid-chain rollback")?;
    }

    let store = GovStore::open(&db_path);
    let mut disbursements = store.disbursements()?;
    disbursements.sort_by_key(|d| d.id);

    let root = disbursements
        .iter()
        .find(|d| d.id == 1)
        .expect("root present")
        .clone();
    let child = disbursements
        .iter()
        .find(|d| d.id == 2)
        .expect("child present")
        .clone();
    let leaf = disbursements
        .iter()
        .find(|d| d.id == 3)
        .expect("leaf present")
        .clone();

    assert_eq!(canonical_dependencies(&child), vec![root.id]);
    assert_eq!(canonical_dependencies(&leaf), vec![child.id]);
    assert!(matches!(
        child.status,
        DisbursementStatus::RolledBack { .. }
    ));

    // Dependency validation after restart should still surface the rollback.
    let leaf_payload = DisbursementPayload {
        proposal: DisbursementProposalMetadata {
            deps: canonical_dependencies(&leaf),
            ..Default::default()
        },
        disbursement: DisbursementDetails {
            destination: leaf.destination.clone(),
            amount: leaf.amount,
            memo: leaf.memo.clone(),
            scheduled_epoch: leaf.scheduled_epoch,
            expected_receipts: leaf.expected_receipts.clone(),
        },
    };
    let error = validate_dependencies(&leaf_payload, &disbursements)
        .expect_err("rolled back dependency should block execution");
    match error {
        the_block::governance::treasury::DisbursementError::DependencyFailed {
            dependency_id,
            ..
        } => assert_eq!(dependency_id, child.id),
        other => panic!("unexpected dependency error: {other:?}"),
    }

    Ok(())
}

#[test]
fn replay_executor_restart_persists_dependency_policy_and_lease() -> TestResult<()> {
    use governance_spec::encode_runtime_backend_policy;
    use the_block::treasury_executor::{memo_dependency_check, spawn_executor, ExecutorParams};

    let dir = tempdir()?;
    let gov_path = dir.path().join("gov.db");
    let mut params = Params::default();
    let mut bc = Blockchain::new(dir.path().join("bc.db").to_string_lossy().as_ref());
    let mut rt = Runtime::new(&mut bc);
    let store = GovStore::open(&gov_path);

    // Apply a dependency policy proposal and ensure it survives reopen.
    let runtime_mask = encode_runtime_backend_policy(["inhouse"]).expect("encode runtime mask");
    let proposal = Proposal {
        id: 0,
        key: ParamKey::RuntimeBackend,
        new_value: runtime_mask,
        min: 1,
        max: (1i64 << governance_spec::RUNTIME_BACKEND_OPTIONS.len()) - 1,
        proposer: "proposer".into(),
        created_epoch: 0,
        vote_deadline_epoch: 1,
        activation_epoch: None,
        status: ProposalStatus::Open,
        deps: Vec::new(),
    };
    let pid = controller::submit_proposal(&store, proposal)?;
    store.vote(
        pid,
        Vote {
            proposal_id: pid,
            voter: "validator-1".into(),
            choice: VoteChoice::Yes,
            weight: 1,
            received_at: 0,
        },
        0,
    )?;
    controller::tally(&store, pid, 3)?;
    controller::activate_ready(&store, 5, &mut rt, &mut params)?;
    assert!(
        !store.dependency_policy_history()?.is_empty(),
        "dependency policy snapshot should be recorded"
    );

    // Queue a dependency chain, execute once, restart executor, and finish replaying state.
    store.record_treasury_accrual(10_000)?;
    let first = store.queue_disbursement(DisbursementPayload {
        proposal: DisbursementProposalMetadata::default(),
        disbursement: DisbursementDetails {
            destination: "tb1replay-first".into(),
            amount: 500,
            memo: "{}".into(),
            scheduled_epoch: 1,
            expected_receipts: Vec::new(),
        },
    })?;
    let _second = store.queue_disbursement(DisbursementPayload {
        proposal: DisbursementProposalMetadata {
            deps: vec![first.id],
            ..Default::default()
        },
        disbursement: DisbursementDetails {
            destination: "tb1replay-second".into(),
            amount: 750,
            memo: format!("depends_on={}", first.id),
            scheduled_epoch: 2,
            expected_receipts: Vec::new(),
        },
    })?;

    let chain = Arc::new(Mutex::new(Blockchain::default()));
    {
        let mut guard = chain.lock().unwrap();
        guard.block_height = 128; // epoch 1
        guard.config.treasury_account = "treasury".into();
        guard.accounts.insert(
            "treasury".into(),
            Account {
                address: "treasury".into(),
                balance: TokenBalance { amount: 20_000 },
                nonce: 0,
                pending_amount: 0,
                pending_nonce: 0,
                pending_nonces: HashSet::new(),
                sessions: Vec::new(),
            },
        );
    }
    let signing_key = the_block::generate_keypair().0;
    let params = ExecutorParams {
        identity: "replay-exec".into(),
        poll_interval: std::time::Duration::from_millis(25),
        lease_ttl: std::time::Duration::from_millis(200),
        signing_key: Arc::new(signing_key),
        treasury_account: "treasury".into(),
        dependency_check: Some(memo_dependency_check()),
    };
    let handle = spawn_executor(&store, Arc::clone(&chain), params);
    std::thread::sleep(std::time::Duration::from_millis(150));
    handle.shutdown();
    handle.join();

    // Restart executor with a fresh store and advance epoch to release the dependency.
    let reopened = GovStore::open(&gov_path);
    {
        let mut guard = chain.lock().unwrap();
        guard.block_height = 256; // epoch 2
    }
    let restarted = spawn_executor(
        &reopened,
        Arc::clone(&chain),
        ExecutorParams {
            identity: "replay-exec-restart".into(),
            poll_interval: std::time::Duration::from_millis(25),
            lease_ttl: std::time::Duration::from_millis(200),
            signing_key: Arc::new(the_block::generate_keypair().0),
            treasury_account: "treasury".into(),
            dependency_check: Some(memo_dependency_check()),
        },
    );

    let mut attempts = 0;
    while attempts < 80 {
        let disbursements = reopened.disbursements()?;
        let executed = disbursements
            .iter()
            .filter(|d| matches!(d.status, DisbursementStatus::Executed { .. }))
            .count();
        if executed == 2 {
            break;
        }
        attempts += 1;
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    restarted.shutdown();
    restarted.join();

    let history = reopened.dependency_policy_history()?;
    assert!(
        !history.is_empty(),
        "dependency policy history should persist across restarts"
    );
    let snapshot = reopened
        .executor_snapshot()?
        .expect("executor snapshot should exist after restart");
    assert!(
        snapshot.last_success_at.is_some(),
        "executor should record successful replay after restart"
    );

    // Final state: both disbursements executed and dependency policy intact.
    let final_state = reopened.disbursements()?;
    assert!(
        final_state
            .iter()
            .filter(|d| matches!(d.status, DisbursementStatus::Executed { .. }))
            .count()
            == 2
    );

    Ok(())
}
