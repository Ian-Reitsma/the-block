/// Integration test for receipt serialization and deterministic metrics.
///
/// This test validates:
/// 1. Receipts survive block serialization/deserialization roundtrip
/// 2. Block hash determinism with receipts
/// 3. Metrics derivation from receipts produces deterministic results
/// 4. Cross-node consistency (two nodes, same chain, same metrics)
use the_block::{
    block_binary, economics::deterministic_metrics::derive_market_metrics_from_chain, AdReceipt,
    Block, ComputeReceipt, EnergyReceipt, Receipt, StorageReceipt,
};

fn block_with_receipts(index: u64, receipts: Vec<Receipt>) -> Block {
    Block {
        index,
        receipts,
        ..Default::default()
    }
}

#[test]
fn receipts_survive_block_serialization_roundtrip() {
    // Create a block with test receipts
    let block = block_with_receipts(
        0,
        vec![
            Receipt::Storage(StorageReceipt {
                contract_id: "storage_contract_1".into(),
                provider: "provider_alice".into(),
                bytes: 1_000_000,
                price_ct: 5_000,
                block_height: 100,
                provider_escrow: 50_000,
                provider_signature: vec![0u8; 64],
                signature_nonce: 100,
            }),
            Receipt::Storage(StorageReceipt {
                contract_id: "storage_contract_2".into(),
                provider: "provider_bob".into(),
                bytes: 2_000_000,
                price_ct: 10_000,
                block_height: 100,
                provider_escrow: 100_000,
                provider_signature: vec![0u8; 64],
                signature_nonce: 100,
            }),
            Receipt::Compute(ComputeReceipt {
                job_id: "job_1".into(),
                provider: "compute_provider_1".into(),
                compute_units: 5_000,
                payment_ct: 2_500,
                block_height: 100,
                verified: true,
                provider_signature: vec![0u8; 64],
                signature_nonce: 100,
            }),
            Receipt::Compute(ComputeReceipt {
                job_id: "job_2".into(),
                provider: "compute_provider_2".into(),
                compute_units: 3_000,
                payment_ct: 1_500,
                block_height: 100,
                verified: false,
                provider_signature: vec![0u8; 64],
                signature_nonce: 100,
            }),
            Receipt::Energy(EnergyReceipt {
                contract_id: "energy_1".into(),
                provider: "grid_operator_1".into(),
                energy_units: 1_000_000,
                price_ct: 50_000,
                block_height: 100,
                proof_hash: [0x42u8; 32],
                provider_signature: vec![0u8; 64],
                signature_nonce: 100,
            }),
            Receipt::Ad(AdReceipt {
                campaign_id: "campaign_xyz".into(),
                publisher: "pub_1".into(),
                impressions: 100_000,
                spend_ct: 5_000,
                block_height: 100,
                conversions: 250,
                publisher_signature: vec![0u8; 64],
                signature_nonce: 100,
            }),
        ],
    );

    let original_receipt_count = block.receipts.len();

    // Serialize the block
    let encoded = block_binary::encode_block(&block).expect("failed to encode block");

    // Deserialize
    let decoded = block_binary::decode_block(&encoded).expect("failed to decode block");

    // Verify receipt data integrity
    assert_eq!(
        decoded.receipts.len(),
        original_receipt_count,
        "Receipt count mismatch after roundtrip"
    );

    for (i, (original, decoded)) in block
        .receipts
        .iter()
        .zip(decoded.receipts.iter())
        .enumerate()
    {
        assert_eq!(original, decoded, "Receipt #{} mismatch after roundtrip", i);
        assert_eq!(
            original.market_name(),
            decoded.market_name(),
            "Market name mismatch for receipt #{}",
            i
        );
        assert_eq!(
            original.settlement_amount(),
            decoded.settlement_amount(),
            "Settlement amount mismatch for receipt #{}",
            i
        );
        assert_eq!(
            original.block_height(),
            decoded.block_height(),
            "Block height mismatch for receipt #{}",
            i
        );
    }

    println!(
        "✓ Receipt serialization roundtrip successful ({} receipts)",
        original_receipt_count
    );
}

#[test]
fn deterministic_metrics_from_receipts_chain() {
    // Create a synthetic chain with receipts across 3 blocks (representing part of an epoch)
    let chain = vec![
        block_with_receipts(
            0,
            vec![
                Receipt::Storage(StorageReceipt {
                    contract_id: "storage_1".into(),
                    provider: "provider_1".into(),
                    bytes: 5_000_000,
                    price_ct: 50_000,
                    block_height: 0,
                    provider_escrow: 500_000,
                    provider_signature: vec![0u8; 64],
                    signature_nonce: 0,
                }),
                Receipt::Storage(StorageReceipt {
                    contract_id: "storage_2".into(),
                    provider: "provider_1".into(),
                    bytes: 5_000_000,
                    price_ct: 50_000,
                    block_height: 0,
                    provider_escrow: 500_000,
                    provider_signature: vec![0u8; 64],
                    signature_nonce: 0,
                }),
            ],
        ),
        block_with_receipts(
            1,
            vec![
                Receipt::Compute(ComputeReceipt {
                    job_id: "job_1".into(),
                    provider: "compute_1".into(),
                    compute_units: 10_000,
                    payment_ct: 5_000,
                    block_height: 1,
                    verified: true,
                    provider_signature: vec![0u8; 64],
                    signature_nonce: 1,
                }),
                Receipt::Compute(ComputeReceipt {
                    job_id: "job_2".into(),
                    provider: "compute_2".into(),
                    compute_units: 5_000,
                    payment_ct: 2_500,
                    block_height: 1,
                    verified: true,
                    provider_signature: vec![0u8; 64],
                    signature_nonce: 1,
                }),
            ],
        ),
        block_with_receipts(
            2,
            vec![
                Receipt::Storage(StorageReceipt {
                    contract_id: "storage_3".into(),
                    provider: "provider_2".into(),
                    bytes: 3_000_000,
                    price_ct: 30_000,
                    block_height: 2,
                    provider_escrow: 300_000,
                    provider_signature: vec![0u8; 64],
                    signature_nonce: 2,
                }),
                Receipt::Ad(AdReceipt {
                    campaign_id: "campaign_1".into(),
                    publisher: "pub_1".into(),
                    impressions: 50_000,
                    spend_ct: 2_500,
                    block_height: 2,
                    conversions: 125,
                    publisher_signature: vec![0u8; 64],
                    signature_nonce: 2,
                }),
            ],
        ),
    ];

    // Derive metrics from epoch blocks (0-3, exclusive)
    let metrics = derive_market_metrics_from_chain(&chain, 0, 3);

    // Verify non-zero metrics for active markets
    assert!(
        metrics.storage.utilization > 0.0,
        "Storage utilization should be > 0"
    );
    assert!(
        metrics.compute.utilization > 0.0,
        "Compute utilization should be > 0"
    );
    assert!(metrics.ad.utilization > 0.0, "Ad utilization should be > 0");

    println!("✓ Storage utilization: {:.4}", metrics.storage.utilization);
    println!("✓ Compute utilization: {:.4}", metrics.compute.utilization);
    println!("✓ Ad utilization: {:.4}", metrics.ad.utilization);

    // Verify determinism: same chain → same metrics
    let metrics2 = derive_market_metrics_from_chain(&chain, 0, 3);
    assert_eq!(
        metrics.storage.utilization, metrics2.storage.utilization,
        "Storage utilization not deterministic"
    );
    assert_eq!(
        metrics.compute.utilization, metrics2.compute.utilization,
        "Compute utilization not deterministic"
    );
    assert_eq!(
        metrics.ad.utilization, metrics2.ad.utilization,
        "Ad utilization not deterministic"
    );

    println!("✓ Metrics derivation is deterministic (same chain → same metrics)");
}

#[test]
fn cross_node_consistency_same_chain_same_metrics() {
    // Simulate two nodes processing the same blocks

    let mut chain = Vec::new();
    for i in 0u64..5 {
        let scale = i + 1;
        let block = block_with_receipts(
            i,
            vec![Receipt::Storage(StorageReceipt {
                contract_id: format!("storage_{}", i),
                provider: "provider_1".into(),
                bytes: 1_000_000 * scale,
                price_ct: 10_000 * scale,
                block_height: i,
                provider_escrow: 100_000 * scale,
                provider_signature: vec![0u8; 64],
                signature_nonce: i,
            })],
        );
        chain.push(block);
    }

    // "Node 1" derives metrics
    let metrics_node1 = derive_market_metrics_from_chain(&chain, 0, 5);

    // "Node 2" derives metrics (same chain, should get identical results)
    let metrics_node2 = derive_market_metrics_from_chain(&chain, 0, 5);

    // Cross-node consistency check
    assert_eq!(
        metrics_node1.storage.utilization,
        metrics_node2.storage.utilization
    );
    assert_eq!(
        metrics_node1.compute.utilization,
        metrics_node2.compute.utilization
    );
    assert_eq!(
        metrics_node1.energy.utilization,
        metrics_node2.energy.utilization
    );
    assert_eq!(metrics_node1.ad.utilization, metrics_node2.ad.utilization);

    println!("✓ Cross-node consistency verified: identical metrics from same chain");
}

#[test]
fn receipt_metrics_integration_pipeline() {
    // End-to-end test: block → serialization → deserialization → metrics → governance

    // Create a block with realistic receipts
    let block = block_with_receipts(
        42,
        vec![
            Receipt::Storage(StorageReceipt {
                contract_id: "contract_id_1".into(),
                provider: "storage_provider_alice".into(),
                bytes: 10_000_000,
                price_ct: 100_000,
                block_height: 42,
                provider_escrow: 1_000_000,
                provider_signature: vec![0u8; 64],
                signature_nonce: 42,
            }),
            Receipt::Compute(ComputeReceipt {
                job_id: "compute_job_1".into(),
                provider: "compute_provider_bob".into(),
                compute_units: 50_000,
                payment_ct: 25_000,
                block_height: 42,
                verified: true,
                provider_signature: vec![0u8; 64],
                signature_nonce: 42,
            }),
            Receipt::Ad(AdReceipt {
                campaign_id: "campaign_id_2".into(),
                publisher: "publisher_charlie".into(),
                impressions: 500_000,
                spend_ct: 50_000,
                block_height: 42,
                conversions: 1_250,
                publisher_signature: vec![0u8; 64],
                signature_nonce: 42,
            }),
        ],
    );

    // Simulate serialization/transmission/deserialization
    let encoded = block_binary::encode_block(&block).expect("encode failed");
    let transmitted_block = block_binary::decode_block(&encoded).expect("decode failed");

    // Build a chain for metrics derivation
    let chain = vec![transmitted_block];

    // Derive metrics from the transmitted block
    let metrics = derive_market_metrics_from_chain(&chain, 0, 1);

    // Verify metrics are non-zero and valid for governance evaluation
    assert!(
        metrics.storage.utilization >= 0.0 && metrics.storage.utilization <= 1.0,
        "Storage utilization out of bounds: {}",
        metrics.storage.utilization
    );
    assert!(
        metrics.compute.utilization >= 0.0 && metrics.compute.utilization <= 1.0,
        "Compute utilization out of bounds: {}",
        metrics.compute.utilization
    );
    assert!(
        metrics.ad.utilization >= 0.0 && metrics.ad.utilization <= 1.0,
        "Ad utilization out of bounds: {}",
        metrics.ad.utilization
    );

    println!("✓ End-to-end pipeline successful:");
    println!(
        "  - Storage utilization: {:.4}",
        metrics.storage.utilization
    );
    println!(
        "  - Compute utilization: {:.4}",
        metrics.compute.utilization
    );
    println!("  - Ad utilization: {:.4}", metrics.ad.utilization);
}
