use the_block::compute_market::dispute::{DisputeController, DisputePolicy, PendingDispute};
use the_block::receipts::{BlockTorchReceiptMetadata, ComputeReceipt};

fn blocktorch_metadata(descriptor: [u8; 32], kernel: [u8; 32]) -> BlockTorchReceiptMetadata {
    BlockTorchReceiptMetadata {
        kernel_variant_digest: kernel,
        descriptor_digest: descriptor,
        output_digest: [0u8; 32],
        benchmark_commit: Some("commit".into()),
        tensor_profile_epoch: Some("epoch".into()),
        proof_latency_ms: 5,
    }
}

fn build_receipt(
    job_id: &str,
    provider: &str,
    units: u64,
    descriptor: [u8; 32],
    kernel: [u8; 32],
) -> ComputeReceipt {
    ComputeReceipt {
        job_id: job_id.to_string(),
        provider: provider.to_string(),
        compute_units: units,
        payment: units,
        block_height: 100,
        verified: true,
        blocktorch: Some(blocktorch_metadata(descriptor, kernel)),
        provider_signature: vec![0u8; 64],
        signature_nonce: 1,
    }
}

fn pending_dispute(
    job_id: &str,
    expected_manifest: [u8; 32],
    expected_workload: [u8; 32],
) -> PendingDispute {
    PendingDispute {
        job_id: job_id.to_string(),
        provider: "provider".into(),
        buyer: "attacker".into(),
        expected_workload_hash: expected_workload,
        expected_manifest_hash: expected_manifest,
        expected_resource_units: 100,
        opened_at: 3,
    }
}

#[test]
fn verification_budget_blocks_massive_collusion() {
    let policy = DisputePolicy {
        timeout_blocks: 5,
        verification_limit: 150,
        verification_window: 10,
    };
    let mut controller = DisputeController::with_policy(policy);
    let dispute_a = pending_dispute("job-a", [1u8; 32], [2u8; 32]);
    let dispute_b = pending_dispute("job-b", [1u8; 32], [2u8; 32]);
    controller.open(dispute_a.clone());
    controller.open(dispute_b.clone());

    let receipt_a = build_receipt(
        "job-a",
        "provider-1",
        120,
        [3u8; 32],
        dispute_a.expected_workload_hash,
    );
    let receipt_b = build_receipt(
        "job-b",
        "provider-2",
        120,
        [4u8; 32],
        dispute_b.expected_workload_hash,
    );

    let slashes = controller.drain_pending(&[receipt_a, receipt_b], 10);
    assert_eq!(slashes.len(), 2);
    assert!(slashes
        .iter()
        .any(|slash| slash.reason == "verification_budget_exceeded"));
    assert!(slashes
        .iter()
        .any(|slash| slash.reason == "manifest_mismatch"));
}
