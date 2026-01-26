use the_block::compute_market::dispute::{DisputeController, PendingDispute};
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

fn pending_dispute(expected_manifest: [u8; 32], expected_workload: [u8; 32]) -> PendingDispute {
    PendingDispute {
        job_id: "job-1".into(),
        provider: "provider-1".into(),
        buyer: "buyer".into(),
        expected_workload_hash: expected_workload,
        expected_manifest_hash: expected_manifest,
        expected_resource_units: 100,
        opened_at: 5,
    }
}

#[test]
fn manifest_mismatch_produces_slash() {
    let mut controller = DisputeController::new(5);
    let dispute = pending_dispute([2u8; 32], [3u8; 32]);
    controller.open(dispute.clone());

    let receipt = build_receipt(
        "job-1",
        "provider-1",
        120,
        [4u8; 32],
        dispute.expected_workload_hash,
    );
    let slashes = controller.drain_pending(&[receipt], 8);

    assert_eq!(slashes.len(), 1);
    let slash = &slashes[0];
    assert_eq!(slash.reason, "manifest_mismatch");
    assert_eq!(slash.deadline, dispute.opened_at + 5);
}

#[test]
fn underreported_units_triggers_slash() {
    let mut controller = DisputeController::new(5);
    let dispute = pending_dispute([1u8; 32], [7u8; 32]);
    controller.open(dispute.clone());

    let receipt = build_receipt(
        "job-1",
        "provider-1",
        80,
        dispute.expected_manifest_hash,
        dispute.expected_workload_hash,
    );
    let slashes = controller.drain_pending(&[receipt], 9);

    assert_eq!(slashes.len(), 1);
    assert_eq!(slashes[0].reason, "underreported_units");
    assert_eq!(slashes[0].deadline, dispute.opened_at + 5);
}

#[test]
fn missing_receipt_slash_after_timeout() {
    let mut controller = DisputeController::new(5);
    let dispute = pending_dispute([1u8; 32], [8u8; 32]);
    controller.open(dispute.clone());

    let slashes = controller.drain_pending(&[], dispute.opened_at + 5);
    assert_eq!(slashes.len(), 1);
    assert_eq!(slashes[0].reason, "missing_receipt");
    assert_eq!(slashes[0].deadline, dispute.opened_at + 5);
}
